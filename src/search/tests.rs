use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::*;
use crate::onion::{
    SECRET_KEY_HEADER, decode_onion_address, is_clamped, remove_test_dir, verify_hidden_service_dir,
};
use crate::pattern::Pattern;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let mut nonce = [0u8; 8];
        getrandom::fill(&mut nonce).unwrap();
        let dir = std::env::temp_dir().join(format!(
            "torv3-vanity-{name}-{}-{:016x}",
            std::process::id(),
            u64::from_le_bytes(nonce)
        ));
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        remove_test_dir(&self.0);
    }
}

fn config(
    patterns: &[&str],
    anywhere: bool,
    contains: &[&str],
    count: u64,
    dir: &TempDir,
) -> SearchConfig {
    let parse = |v: &[&str]| v.iter().map(|s| Pattern::parse(s).unwrap()).collect();
    SearchConfig {
        matcher: Matcher::new(parse(patterns), anywhere, parse(contains)).unwrap(),
        count: Some(count),
        output_dir: dir.0.clone(),
        cpu_threads: 2,
        gpu: GpuMode::Off,
        gpu_api: GpuApi::Auto,
    }
}

/// Run a search to completion and return what it found.
fn run(config: SearchConfig) -> (Vec<Found>, Vec<Event>) {
    let search = Search::start(config).unwrap();
    let deadline = Instant::now() + Duration::from_secs(120);
    let (mut found, mut others) = (Vec::new(), Vec::new());
    loop {
        assert!(Instant::now() < deadline, "search took too long");
        match search.next_event(Duration::from_millis(100)) {
            Some(Event::Found(f)) => found.push(f),
            Some(Event::Finished) => break,
            Some(other) => others.push(other),
            None => {}
        }
    }
    assert!(search.is_complete(), "search ended early: {others:?}");
    (found, others)
}

fn read_secret_scalar(found: &Found) -> [u8; 32] {
    let bytes = std::fs::read(found.path.join("hs_ed25519_secret_key")).unwrap();
    assert_eq!(&bytes[..32], SECRET_KEY_HEADER);
    bytes[32..64].try_into().unwrap()
}

#[test]
fn finds_prefixes_and_writes_valid_keys() {
    let dir = TempDir::new("prefix");
    let (found, others) = run(config(&["ab", "z?"], false, &[], 3, &dir));
    assert!(others.is_empty(), "unexpected events: {others:?}");
    assert_eq!(found.len(), 6);
    for pattern in ["ab", "z?"] {
        assert_eq!(found.iter().filter(|f| f.pattern == pattern).count(), 3);
    }

    let mut scalars = HashSet::new();
    for f in &found {
        let host = f.address.trim_end_matches(".onion");
        let p = Pattern::parse(&f.pattern).unwrap();
        assert!(
            p.matches_start(host.as_bytes()),
            "{} !~ {}",
            f.address,
            f.pattern
        );
        assert!(decode_onion_address(&f.address).is_ok());
        assert_eq!(verify_hidden_service_dir(&f.path).unwrap(), f.address);
        assert!(f.path.ends_with(host));
        let scalar = read_secret_scalar(f);
        assert!(is_clamped(&scalar));
        assert!(scalars.insert(scalar), "duplicate key");
    }
}

#[test]
fn saved_keys_come_from_independent_walks() {
    // Every saved key restarts its worker from a fresh random key, so no two
    // saved keys may differ by a small multiple of 8.
    let dir = TempDir::new("unrelated");
    let (found, _) = run(config(&["a"], false, &[], 12, &dir));
    let scalars: Vec<[u8; 32]> = found.iter().map(read_secret_scalar).collect();
    for (i, a) in scalars.iter().enumerate() {
        for b in &scalars[i + 1..] {
            let (lo_a, lo_b) = (
                u128::from_le_bytes(a[..16].try_into().unwrap()),
                u128::from_le_bytes(b[..16].try_into().unwrap()),
            );
            assert!(a[16..] != b[16..] || lo_a.abs_diff(lo_b) > 1 << 40);
        }
    }
}

#[test]
fn anywhere_mode() {
    let dir = TempDir::new("anywhere");
    let (found, _) = run(config(&["kv"], true, &[], 2, &dir));
    assert_eq!(found.len(), 2);
    for f in &found {
        assert!(f.address.trim_end_matches(".onion").contains("kv"));
        assert_eq!(verify_hidden_service_dir(&f.path).unwrap(), f.address);
    }
}

#[test]
fn contains_filter() {
    let dir = TempDir::new("contains");
    let (found, _) = run(config(&["a"], false, &["zz"], 1, &dir));
    assert_eq!(found.len(), 1);
    let host = found[0].address.trim_end_matches(".onion");
    assert!(host.starts_with('a') && host.contains("zz"), "{host}");
}

#[test]
fn stops_on_request_and_reports_stats() {
    let dir = TempDir::new("stop");
    let search = Search::start(config(&["abcdefghij"], false, &[], 1, &dir)).unwrap();
    std::thread::sleep(Duration::from_millis(500));
    search.stop_handle().stop();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !search.is_finished() {
        assert!(Instant::now() < deadline, "workers did not stop");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!search.is_complete());
    let stats = search.stats();
    assert!(stats.keys_checked > 0);
    assert_eq!(stats.found, 0);
    // Finished is always the last event.
    let mut last = None;
    while let Some(e) = search.next_event(Duration::from_millis(100)) {
        last = Some(e);
    }
    assert_eq!(last, Some(Event::Finished));
    assert!(!dir.0.exists() || std::fs::read_dir(&dir.0).unwrap().next().is_none());
}

#[test]
fn unwritable_output_is_rejected_before_searching() {
    let dir = TempDir::new("unwritable");
    // A regular file where the output directory should be.
    std::fs::write(&dir.0, b"not a directory").unwrap();
    assert!(matches!(
        Search::start(config(&["a"], false, &[], 1, &dir)),
        Err(SearchError::OutputDir { .. })
    ));
    let mut nested = config(&["a"], false, &[], 1, &dir);
    nested.output_dir = dir.0.join("keys");
    assert!(matches!(
        Search::start(nested),
        Err(SearchError::OutputDir { .. })
    ));
    let _ = std::fs::remove_file(&dir.0);
}

/// Wait for the next event other than `Info`.
fn next_important(search: &Search, timeout: Duration) -> Option<Event> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        match search.next_event(Duration::from_millis(50)) {
            Some(Event::Info(_)) | None => {}
            other => return other,
        }
    }
    None
}

#[test]
fn failed_save_keeps_the_key_and_retries() {
    let dir = TempDir::new("retry");
    let search = Search::start(config(&["abcd"], false, &[], 1, &dir)).unwrap();
    // Break the output folder right after the start-up check.
    std::fs::remove_dir_all(&dir.0).unwrap();
    std::fs::write(&dir.0, b"not a directory").unwrap();

    let warning = next_important(&search, Duration::from_secs(120));
    assert!(
        matches!(&warning, Some(Event::Warning(m)) if m.contains("Retrying")),
        "{warning:?}"
    );
    // Nothing else happens while the problem lasts: no related keys, no
    // second warning.
    assert_eq!(next_important(&search, Duration::from_secs(1)), None);

    // Fix it: the same key gets saved.
    std::fs::remove_file(&dir.0).unwrap();
    std::fs::create_dir(&dir.0).unwrap();
    let found = match next_important(&search, Duration::from_secs(30)) {
        Some(Event::Found(found)) => found,
        other => panic!("expected the key to be saved, got {other:?}"),
    };
    assert_eq!(
        verify_hidden_service_dir(&found.path).unwrap(),
        found.address
    );
    assert_eq!(
        next_important(&search, Duration::from_secs(30)),
        Some(Event::Finished)
    );
    assert!(search.is_complete());
    assert_eq!(std::fs::read_dir(&dir.0).unwrap().count(), 1);
}

#[test]
fn stopping_while_a_save_fails_is_an_error() {
    let dir = TempDir::new("lost");
    let search = Search::start(config(&["abcd"], false, &[], 1, &dir)).unwrap();
    std::fs::remove_dir_all(&dir.0).unwrap();
    std::fs::write(&dir.0, b"not a directory").unwrap();
    assert!(matches!(
        next_important(&search, Duration::from_secs(120)),
        Some(Event::Warning(_))
    ));
    search.stop();
    assert!(matches!(
        next_important(&search, Duration::from_secs(10)),
        Some(Event::Error(m)) if m.contains("lost")
    ));
    assert_eq!(
        next_important(&search, Duration::from_secs(10)),
        Some(Event::Finished)
    );
    assert!(!search.is_complete());
    assert_eq!(search.stats().found, 0);
    let _ = std::fs::remove_file(&dir.0);
}

#[test]
fn events_end_after_finished() {
    let dir = TempDir::new("finished");
    let search = Search::start(config(&["a"], false, &[], 1, &dir)).unwrap();
    assert!(matches!(
        next_important(&search, Duration::from_secs(60)),
        Some(Event::Found(_))
    ));
    assert_eq!(
        next_important(&search, Duration::from_secs(60)),
        Some(Event::Finished)
    );
    // The channel is closed, so this returns at once instead of waiting.
    let started = Instant::now();
    assert_eq!(search.next_event(Duration::from_secs(30)), None);
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn rejects_invalid_configs() {
    let dir = TempDir::new("invalid");
    let mut c = config(&["a"], false, &[], 1, &dir);
    c.count = Some(0);
    assert!(matches!(Search::start(c), Err(SearchError::InvalidCount)));
    #[cfg(feature = "gpu")]
    for (patterns, anywhere) in [
        (&["ab"][..], true),
        (&[&*format!("{}a", "?".repeat(51))][..], false),
    ] {
        let mut c = config(patterns, anywhere, &[], 1, &dir);
        c.cpu_threads = 0;
        c.gpu = GpuMode::On;
        assert!(matches!(Search::start(c), Err(SearchError::NeedsCpu)));
    }
}

#[test]
fn rejects_configs_without_workers() {
    let dir = TempDir::new("noworkers");
    let mut c = config(&["a"], false, &[], 1, &dir);
    c.cpu_threads = 0;
    assert!(matches!(Search::start(c), Err(SearchError::NoWorkers)));
}

#[test]
fn scalar_addition() {
    let mut base = [0u8; 32];
    base[..8].copy_from_slice(&u64::MAX.to_le_bytes());
    let sum = scalar_add(&base, 1);
    assert_eq!(sum[..8], [0; 8]);
    assert_eq!(sum[8], 1);
    let mut top = [0xffu8; 32];
    top[31] = 0x3f;
    let sum = scalar_add(&top, 1);
    assert_eq!(sum[31], 0x40);
    assert!(sum[..31].iter().all(|&b| b == 0));
}

#[test]
fn walk_scalars_step_by_eight() {
    let walk = Walk::random().unwrap();
    let a = curve25519_dalek::Scalar::from_bytes_mod_order(walk.scalar(0));
    let b = curve25519_dalek::Scalar::from_bytes_mod_order(walk.scalar(1000));
    assert_eq!(b - a, curve25519_dalek::Scalar::from(8000u32));
    assert!(is_clamped(&walk.scalar(123_456)));
}

#[test]
fn rate_meter() {
    let mut meter = RateMeter::new(Duration::from_secs(5));
    assert_eq!(meter.update(0), 0.0);
    std::thread::sleep(Duration::from_millis(50));
    let rate = meter.update(1000);
    assert!(rate > 1000.0 && rate < 100_000.0, "{rate}");
}
