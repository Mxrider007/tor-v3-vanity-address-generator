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
    let patterns = ["a", "q?", "?7", "zz", "kor"].map(|p| Pattern::parse(p).unwrap());
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

fn test_gpu(threads: u32, filters: usize) -> Gpu {
    let adapter = choose(GpuMode::On, true).expect(
        "no GPU found (on Linux, install mesa-vulkan-drivers for the lavapipe software GPU)",
    );
    eprintln!("testing on {:?}", adapter.get_info());
    Gpu::new(&adapter, threads, filters).unwrap()
}

impl Gpu {
    fn read_centers(&self, threads: u32) -> Vec<u32> {
        let size = u64::from(threads) * 64;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_buffer_to_buffer(&self.centers, 0, &buffer, 0, size);
        self.queue.submit([encoder.finish()]);
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, |r| r.unwrap());
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        let data = buffer.slice(..).get_mapped_range().unwrap();
        data.as_chunks::<4>()
            .0
            .iter()
            .map(|c| u32::from_le_bytes(*c))
            .collect()
    }
}

/// Run the real kernel and compare every candidate and the final centers
/// with the CPU walker.
#[test]
#[ignore = "needs a GPU or lavapipe; run with `cargo test -- --ignored`"]
fn kernel_matches_cpu_walker() {
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

    let gpu = test_gpu(threads, filters.len());
    gpu.upload_centers(&walk.centers(threads));
    gpu.upload_filters(&filters);
    let hits = gpu
        .dispatch(groups, threads, filters.len() as u32, 2)
        .unwrap();

    let got: HashSet<Hit> = hits.into_iter().collect();
    let want: HashSet<Hit> = targets.iter().map(|(h, _)| *h).collect();
    assert_eq!(got, want);
    assert_eq!(gpu.read_centers(threads), next_centers);
}

/// A complete search on the GPU alone; every key is verified by the CPU.
#[test]
#[ignore = "needs a GPU or lavapipe; run with `KORE_VANITY_SOFTWARE_GPU=1 cargo test -- --ignored`"]
fn gpu_only_search_finds_keys() {
    let dir = std::env::temp_dir().join(format!("kore-vanity-gpu-{}", std::process::id()));
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
