use std::collections::HashSet;
use std::time::{Duration, Instant};

use super::*;
use crate::curve::walk::{Center, GroupWalker};
use crate::onion::SecretKey;
use crate::pattern::{Matcher, Pattern};
use crate::search::{Event, Search, SearchConfig};

/// The generated WGSL must pass naga's validator (this runs without a GPU).
#[test]
fn shader_validates() {
    use wgpu::naga;
    for group in [4, GROUP] {
        let source = shader::source(group);
        let module = naga::front::wgsl::parse_str(&source)
            .unwrap_or_else(|e| panic!("{}", e.emit_to_string(&source)));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap_or_else(|e| panic!("{}", e.emit_to_string(&source)));
    }
}

/// The GPU filter words select exactly the keys the CPU filter selects.
#[test]
fn filter_words_agree_with_prefix_filter() {
    let patterns = ["a", "q?", "?7", "zz", "sho"].map(|p| Pattern::parse(p).unwrap());
    let mut hits = 0;
    for _ in 0..3000 {
        let mut seed = [0u8; 32];
        getrandom::fill(&mut seed).unwrap();
        let mut y = SecretKey::from_seed(&seed).public_key();
        y[31] &= 0x7f;
        let words = shader::words(&y);
        for p in &patterns {
            let filter = p.prefix_filter();
            let gpu = filter_words(&filter);
            let gpu_match = (0..8).all(|w| words[w] & gpu[w] == gpu[8 + w]);
            assert_eq!(gpu_match, filter.test(&y));
            hits += usize::from(gpu_match);
        }
    }
    assert!(hits > 100);
}

#[test]
fn walk_key_indices() {
    let walk = GpuWalk::new().unwrap();
    let hit = |thread, group, index| Hit {
        thread,
        group,
        index,
    };
    assert_eq!(walk.key(hit(0, 0, 0), 1).unwrap(), GROUP as u64);
    assert_eq!(walk.key(hit(0, 0, 2 * GROUP as u32), 1).unwrap(), 0);
    assert_eq!(
        walk.key(hit(2, 1, 1), 4).unwrap(),
        2 * GpuWalk::KEYS_PER_THREAD + GROUP as u64 + KEYS_PER_GROUP + 1
    );
    assert!(walk.key(hit(0, 4, 0), 4).is_err());
    assert!(walk.key(hit(0, 0, KEYS_PER_GROUP as u32), 1).is_err());
}

#[test]
fn hit_list_is_decoded() {
    let mut words = vec![0u32; 1 + 3 * MAX_HITS];
    words[..7].copy_from_slice(&[2, 5, 1, 7, 9, 0, 128]);
    let (hits, total) = parse_hits(&words);
    assert_eq!(total, 2);
    assert_eq!(
        hits,
        [
            Hit {
                thread: 5,
                group: 1,
                index: 7
            },
            Hit {
                thread: 9,
                group: 0,
                index: 128
            }
        ]
    );
    // Only MAX_HITS are recorded, however many there were.
    words[0] = 5000;
    let (hits, total) = parse_hits(&words);
    assert_eq!((hits.len(), total), (MAX_HITS, 5000));
}

/// A wgpu device (a real GPU, or Mesa's lavapipe software GPU).
fn test_gpu(threads: u32, filters: usize) -> Gpu {
    let adapter = wgpu_adapters(GpuMode::On, true, GpuApi::Auto)
        .into_iter()
        .next()
        .expect(
            "no GPU found (on Linux, install mesa-vulkan-drivers for the lavapipe software GPU)",
        );
    eprintln!("testing on {:?}", adapter.get_info());
    Gpu::new(&adapter, threads, filters).unwrap()
}

/// Run the kernel and compare every candidate and the final centers with the
/// CPU walker.
fn check_kernel_matches_cpu_walker(new_gpu: impl FnOnce(u32, usize) -> Box<dyn Device>) {
    let (threads, groups) = (3u32, 2u32);
    let table = WalkTable::new(GROUP);
    let walk = GpuWalk::new().unwrap();

    // CPU reference: every key's y and each invocation's next center.
    let mut expected = Vec::new();
    let mut next_centers = Vec::new();
    for t in 0..threads {
        let start = ExtendedPoint::mul_base(&walk.walk.scalar(GpuWalk::first_center(t)));
        let mut walker = GroupWalker::new(&table, Center::from_extended(&start));
        for g in 0..groups {
            walker.step();
            for (index, y) in walker.ys.iter().enumerate() {
                expected.push((
                    Hit {
                        thread: t,
                        group: g,
                        index: index as u32,
                    },
                    *y,
                ));
            }
        }
        let (x, y) = walker.next_center_bytes();
        next_centers.extend(shader::words(&x));
        next_centers.extend(shader::words(&y));
    }

    // Look for every 23rd key using its first 64 bits of y.
    let targets: Vec<&(Hit, [u8; 32])> = expected.iter().step_by(23).collect();
    let filters: Vec<[u32; 16]> = targets
        .iter()
        .map(|(_, y)| {
            let w = shader::words(y);
            let mut f = [0u32; 16];
            f[..2].copy_from_slice(&[u32::MAX, u32::MAX]);
            f[8..10].copy_from_slice(&w[..2]);
            f
        })
        .collect();

    let gpu = new_gpu(threads, filters.len());
    gpu.upload_centers(&walk.centers(threads)).unwrap();
    gpu.upload_filters(&filters).unwrap();
    let (hits, total) = gpu
        .dispatch(groups, threads, filters.len() as u32, 2)
        .unwrap();
    assert_eq!(total as usize, hits.len());

    let got: HashSet<Hit> = hits.into_iter().collect();
    let want: HashSet<Hit> = targets.iter().map(|(h, _)| *h).collect();
    assert_eq!(got, want);
    assert_eq!(gpu.read_centers(threads).unwrap(), next_centers);
}

/// The start-up self-test passes on a working GPU, and catches a broken one.
fn check_self_test_detects_wrong_results(gpu: &dyn Device) {
    self_test(gpu).unwrap();
    // Wrong arithmetic gives wrong points, like a slightly wrong start point.
    assert!(check_against_cpu(gpu, |centers| centers[8] ^= 1 << 4).is_err());
    self_test(gpu).unwrap();
}

/// More candidates than MAX_HITS: only MAX_HITS are recorded.
fn check_hit_overflow_is_reported(gpu: &dyn Device, threads: u32) {
    let walk = GpuWalk::new().unwrap();
    gpu.upload_centers(&walk.centers(threads)).unwrap();
    // Match every key whose y starts with a 0 bit: about half of them.
    let mut filter = [0u32; 16];
    filter[0] = 1;
    gpu.upload_filters(&[filter]).unwrap();
    let (hits, total) = gpu.dispatch(1, threads, 1, 1).unwrap();
    assert_eq!(hits.len(), MAX_HITS);
    assert!(total as usize > MAX_HITS, "{total}");
}

#[test]
#[ignore = "needs a GPU or lavapipe; run with `cargo test -- --ignored`"]
fn kernel_matches_cpu_walker() {
    check_kernel_matches_cpu_walker(|threads, filters| Box::new(test_gpu(threads, filters)));
}

#[test]
#[ignore = "needs a GPU or lavapipe; run with `cargo test -- --ignored`"]
fn self_test_detects_wrong_results() {
    check_self_test_detects_wrong_results(&test_gpu(SELF_TEST_THREADS, 1));
}

#[test]
#[ignore = "needs a GPU or lavapipe; run with `cargo test -- --ignored`"]
fn hit_overflow_is_reported() {
    check_hit_overflow_is_reported(&test_gpu(64, 1), 64);
}

/// The CUDA kernel, compiled for the CPU (`--features kernel-test`).
#[cfg(feature = "kernel-test")]
mod cuda_kernel {
    use super::*;
    use crate::curve::field::Fe;
    use crate::search::gpu::cuda::host::{FieldOp, HostKernel, field_op};

    #[test]
    fn kernel_matches_cpu_walker() {
        check_kernel_matches_cpu_walker(|threads, filters| {
            Box::new(HostKernel::new(threads, filters))
        });
    }

    #[test]
    fn self_test_detects_wrong_results() {
        check_self_test_detects_wrong_results(&HostKernel::new(SELF_TEST_THREADS, 1));
    }

    #[test]
    fn hit_overflow_is_reported() {
        check_hit_overflow_is_reported(&HostKernel::new(64, 1), 64);
    }

    /// A canonical field element as packed words.
    fn words(f: Fe) -> [u32; 8] {
        shader::words(&f.to_bytes())
    }

    fn random_fe() -> Fe {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).unwrap();
        bytes[31] &= 0x7f;
        // Reduce values in [p, 2^255) like the kernel's inputs never are.
        Fe::from_bytes(&Fe::from_bytes(&bytes).to_bytes())
    }

    /// Values near the edges of the representation.
    fn special() -> Vec<Fe> {
        let p_minus = |k: u8| {
            let mut b = [0xffu8; 32];
            b[31] = 0x7f;
            b[0] = 0xed - k;
            Fe::from_bytes(&b)
        };
        let mut v = vec![Fe::ZERO, Fe::ONE, p_minus(1), p_minus(2), p_minus(19)];
        for limb in 0..10usize {
            // All bits of one 26/25-bit limb set, and only its top bit.
            let pos = 26 * limb.div_ceil(2) + 25 * (limb / 2);
            let width = if limb % 2 == 1 { 25 } else { 26 };
            for bits in [
                (pos..pos + width).collect::<Vec<_>>(),
                vec![pos + width - 1],
            ] {
                let mut b = [0u8; 32];
                for bit in bits {
                    b[bit / 8] |= 1 << (bit % 8);
                }
                b[31] &= 0x7f;
                v.push(Fe::from_bytes(&b));
            }
        }
        let mut all_ones = [0xffu8; 32];
        all_ones[31] = 0x7f;
        v.push(Fe::from_bytes(&all_ones));
        v
    }

    fn check(a: Fe, b: Fe) {
        let (wa, wb) = (words(a), words(b));
        assert_eq!(field_op(FieldOp::Mul, &wa, &wb), words(a.mul(b)));
        assert_eq!(field_op(FieldOp::Square, &wa, &wb), words(a.square()));
        assert_eq!(
            field_op(FieldOp::SumTimesDifference, &wa, &wb),
            words(a.add(b).mul(a.sub(b)))
        );
    }

    #[test]
    fn field_arithmetic_matches_cpu() {
        let special = special();
        for &a in &special {
            for &b in &special {
                check(a, b);
            }
        }
        for _ in 0..20_000 {
            check(random_fe(), random_fe());
        }
    }

    #[test]
    fn inversion_matches_cpu() {
        for a in special().into_iter().chain((0..200).map(|_| random_fe())) {
            assert_eq!(
                field_op(FieldOp::Invert, &words(a), &[0; 8]),
                words(a.invert())
            );
        }
    }

    /// Inputs in [p, 2^255) are not canonical; the kernel never receives
    /// them, but it must still reduce its results fully.
    #[test]
    fn results_are_canonical() {
        // p - 1 squared is 1; (p - 1)·(p - 1) + 0 must come out as exactly 1.
        let mut p_minus_1 = [0xffu8; 32];
        p_minus_1[31] = 0x7f;
        p_minus_1[0] = 0xec;
        let w = shader::words(&p_minus_1);
        let mut one = [0u32; 8];
        one[0] = 1;
        assert_eq!(field_op(FieldOp::Mul, &w, &w), one);
        assert_eq!(field_op(FieldOp::Square, &w, &w), one);
    }
}

/// A complete search on the GPU alone; every key is verified by the CPU.
#[test]
#[ignore = "needs a GPU or lavapipe; run with `TORV3_VANITY_SOFTWARE_GPU=1 cargo test -- --ignored`"]
fn gpu_only_search_finds_keys() {
    let dir = std::env::temp_dir().join(format!("torv3-vanity-gpu-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let matcher = Matcher::new(
        vec![Pattern::parse("k2").unwrap(), Pattern::parse("z").unwrap()],
        false,
        vec![],
    )
    .unwrap();
    let search = Search::start(SearchConfig {
        matcher,
        count: Some(2),
        output_dir: dir.clone(),
        cpu_threads: 0,
        gpu: GpuMode::On,
        gpu_api: GpuApi::Auto,
    })
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(300);
    let mut found = Vec::new();
    loop {
        assert!(Instant::now() < deadline, "GPU search took too long");
        match search.next_event(Duration::from_millis(200)) {
            Some(Event::Found(f)) => found.push(f),
            Some(Event::Error(e)) => panic!("{e}"),
            Some(Event::Finished) => break,
            _ => {}
        }
    }
    assert!(search.is_complete());
    assert_eq!(found.len(), 4);
    assert!(search.stats().gpu_keys_checked > 0);
    for f in &found {
        assert!(
            Pattern::parse(&f.pattern)
                .unwrap()
                .matches_start(f.address.as_bytes())
        );
        assert_eq!(
            crate::onion::verify_hidden_service_dir(&f.path).unwrap(),
            f.address
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}
