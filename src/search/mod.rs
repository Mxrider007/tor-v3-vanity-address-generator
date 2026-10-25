//! The vanity search engine.
//!
//! [`Search::start`] launches CPU worker threads (and, with the `gpu`
//! feature, a GPU worker) and returns immediately; progress is reported
//! through [`Search::stats`] and a stream of [`Event`]s.
//!
//! # How keys are generated
//!
//! Each worker picks a fresh random Ed25519 key `a` (from the OS random
//! number generator) and walks the public keys `(a + 8k)·B` for
//! `k = 0, 1, 2, …` using one point addition per key, the same technique
//! used by mkp224o. Adding multiples of 8 keeps the scalar clamped, so every
//! result is a normal-looking Ed25519 key that Tor and other tools accept.
//!
//! Whatever the fast path reports is treated as a *candidate*: the key is
//! re-derived from scratch with curve25519-dalek, re-matched, and must pass a
//! signing self-test before it is written; the files are then read back and
//! verified before they get their final name. After a key is saved, the
//! worker starts over from a new random key, so saved keys are never related
//! to each other. If a key cannot be saved (for example because the disk is
//! full), the worker keeps it in memory and retries until it succeeds or the
//! search is stopped; it never moves on to related keys.

mod cpu;
#[cfg(feature = "gpu")]
mod gpu;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use thiserror::Error;

use crate::curve::walk::WalkTable;
use crate::onion::{
    SecretKey, address_chars, check_output_dir, is_clamped, onion_address, write_hidden_service_dir,
};
use crate::pattern::{MatchResult, Matcher, PrefixFilter};

/// Whether to use a GPU.
///
/// The GPU only searches for patterns at the start of the address. Whenever
/// it cannot be used and there are CPU threads, the search continues on the
/// CPU alone, with an [`Event::Warning`] in `On` mode or when a
/// [`GpuApi`] other than `Auto` was chosen, and an [`Event::Info`]
/// otherwise. With no CPU threads, it stops with an [`Event::Error`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GpuMode {
    /// Use a dedicated GPU (or Apple Silicon graphics) if one is found.
    #[default]
    Auto,
    /// Use the best available GPU, including integrated graphics.
    On,
    /// CPU only.
    Off,
}

/// Which GPU programming interface to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GpuApi {
    /// CUDA on NVIDIA cards, then Vulkan, Metal or DirectX 12; the first
    /// that works.
    #[default]
    Auto,
    /// CUDA (NVIDIA cards only).
    Cuda,
    /// Vulkan (Windows and Linux).
    Vulkan,
    /// DirectX 12 (Windows).
    Dx12,
    /// Metal (macOS).
    Metal,
}

/// What to search for and how.
#[derive(Debug, Clone)]
pub struct SearchConfig {
    /// The patterns and matching rules.
    pub matcher: Matcher,
    /// Addresses to find per pattern (at least 1); `None` keeps going until
    /// stopped.
    pub count: Option<u64>,
    /// Where hidden-service directories are written. It is created if
    /// missing and checked for write access before the search starts.
    pub output_dir: PathBuf,
    /// Number of CPU worker threads (may be 0 when a GPU is used).
    pub cpu_threads: usize,
    /// GPU usage.
    pub gpu: GpuMode,
    /// GPU programming interface.
    pub gpu_api: GpuApi,
}

/// A saved vanity address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// The pattern it matched.
    pub pattern: String,
    /// The full `….onion` address.
    pub address: String,
    /// The hidden-service directory containing the keys.
    pub path: PathBuf,
}

/// Something that happened during a search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A matching address was found, verified and saved.
    Found(Found),
    /// Informational message (for example which GPU is in use).
    Info(String),
    /// Something did not work but the search continues.
    Warning(String),
    /// A fatal problem; the search stops.
    Error(String),
    /// All workers have exited. Always the last event.
    Finished,
}

/// Errors that prevent a search from starting.
#[derive(Debug, Error)]
pub enum SearchError {
    #[error("nothing to search with: use at least one CPU thread or a GPU")]
    NoWorkers,
    #[error("this build of torv3-vanity has no GPU support")]
    NoGpuSupport,
    #[error(
        "the GPU only searches for text at the start of the address (within the first 51 \
         characters); use at least one CPU thread for this search"
    )]
    NeedsCpu,
    #[error("the number of addresses to find must be at least 1")]
    InvalidCount,
    #[error("cannot save keys in {}: {source}", path.display())]
    OutputDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not start a worker thread: {0}")]
    Thread(#[from] std::io::Error),
}

/// A snapshot of search progress.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Stats {
    /// Total keys checked so far.
    pub keys_checked: u64,
    /// Keys checked by the GPU (included in `keys_checked`).
    pub gpu_keys_checked: u64,
    /// Addresses found and saved.
    pub found: u64,
    /// Addresses that matched a pattern but lacked a "contains" word.
    pub near_misses: u64,
    /// Time since the search started.
    pub elapsed: Duration,
}

/// Information about a GPU.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuInfo {
    pub name: String,
    /// Graphics API used, e.g. "Vulkan" or "Dx12".
    pub backend: String,
    /// Whether it is a dedicated graphics card.
    pub discrete: bool,
    /// Whether it is a software (CPU-emulated) device.
    pub software: bool,
    /// Whether [`GpuMode::Auto`] uses it: dedicated cards and Apple Silicon.
    pub preferred: bool,
}

/// Set this environment variable (to anything but `0`) to allow software
/// GPUs such as Mesa's lavapipe. Only useful for testing.
pub const SOFTWARE_GPU_ENV: &str = "TORV3_VANITY_SOFTWARE_GPU";

/// Whether [`SOFTWARE_GPU_ENV`] is set.
pub fn software_gpu_allowed() -> bool {
    std::env::var_os(SOFTWARE_GPU_ENV).is_some_and(|v| !v.is_empty() && v != "0")
}

/// Whether this build can use a GPU (the `gpu` feature).
pub fn gpu_support() -> bool {
    cfg!(feature = "gpu")
}

/// Whether this build can use CUDA (the `cuda` feature).
pub fn cuda_support() -> bool {
    cfg!(feature = "cuda")
}

/// List GPUs usable for the search (empty without the `gpu` feature).
pub fn list_gpus() -> Vec<GpuInfo> {
    #[cfg(feature = "gpu")]
    {
        gpu::list()
    }
    #[cfg(not(feature = "gpu"))]
    {
        Vec::new()
    }
}

/// The number of logical CPUs.
pub fn available_threads() -> usize {
    std::thread::available_parallelism().map_or(1, |n| n.get())
}

/// A running search. Dropping it stops the search.
pub struct Search {
    shared: Arc<Shared>,
    events: Receiver<Event>,
    supervisor: Option<JoinHandle<()>>,
}

/// A cloneable handle that can stop a search from another thread (for
/// example a Ctrl-C handler).
#[derive(Clone)]
pub struct StopHandle(Arc<Shared>);

impl StopHandle {
    pub fn stop(&self) {
        self.0.request_stop();
    }
}

impl Search {
    /// Start searching in the background.
    pub fn start(config: SearchConfig) -> Result<Search, SearchError> {
        let use_gpu = match config.gpu {
            GpuMode::Off => false,
            _ if cfg!(feature = "gpu") => true,
            GpuMode::On => return Err(SearchError::NoGpuSupport),
            GpuMode::Auto => false,
        };
        if config.cpu_threads == 0 {
            if !use_gpu {
                return Err(SearchError::NoWorkers);
            }
            let matcher = &config.matcher;
            if matcher.anywhere()
                || matcher
                    .patterns()
                    .iter()
                    .any(|p| p.prefix_filter().words == 0)
            {
                return Err(SearchError::NeedsCpu);
            }
        }
        if config.count == Some(0) {
            return Err(SearchError::InvalidCount);
        }
        // Find out now, not hours later when the first key is found.
        check_output_dir(&config.output_dir).map_err(|source| SearchError::OutputDir {
            path: config.output_dir.clone(),
            source,
        })?;

        let (tx, events) = mpsc::channel();
        let shared = Arc::new(Shared::new(&config, tx));
        let spawn = || -> std::io::Result<JoinHandle<()>> {
            let mut workers = Vec::new();
            for i in 0..config.cpu_threads {
                workers.push(spawn_worker(&shared, format!("vanity-cpu-{i}"), cpu::run)?);
            }
            #[cfg(feature = "gpu")]
            if use_gpu {
                let (mode, api, cpu_threads) = (config.gpu, config.gpu_api, config.cpu_threads);
                workers.push(spawn_worker(&shared, "vanity-gpu".into(), move |shared| {
                    gpu::run(shared, mode, api, cpu_threads)
                })?);
            }
            let shared = Arc::clone(&shared);
            std::thread::Builder::new()
                .name("vanity-supervisor".into())
                .spawn(move || {
                    for worker in workers {
                        // A panic was already reported by the worker's guard.
                        let _ = worker.join();
                    }
                    shared.finish();
                })
        };
        match spawn() {
            Ok(supervisor) => Ok(Search {
                shared,
                events,
                supervisor: Some(supervisor),
            }),
            Err(e) => {
                // Stop the workers that did start.
                shared.request_stop();
                Err(e.into())
            }
        }
    }

    /// Ask all workers to stop. Returns immediately; a [`Event::Finished`]
    /// event follows once they have.
    pub fn stop(&self) {
        self.shared.request_stop();
    }

    /// A handle that can stop this search from elsewhere.
    pub fn stop_handle(&self) -> StopHandle {
        StopHandle(Arc::clone(&self.shared))
    }

    /// Current progress.
    pub fn stats(&self) -> Stats {
        self.shared.stats()
    }

    /// Next event, if one is waiting.
    pub fn try_event(&self) -> Option<Event> {
        self.events.try_recv().ok()
    }

    /// Wait up to `timeout` for the next event. Returns `None` on timeout, or
    /// immediately once the search has finished and every event was
    /// delivered.
    pub fn next_event(&self, timeout: Duration) -> Option<Event> {
        self.events.recv_timeout(timeout).ok()
    }

    /// Whether every worker has exited.
    pub fn is_finished(&self) -> bool {
        self.supervisor.as_ref().is_none_or(|h| h.is_finished())
    }

    /// Whether every requested address was found and saved (as opposed to
    /// stopped).
    pub fn is_complete(&self) -> bool {
        self.shared.remaining.load(Ordering::Acquire) == 0
    }
}

impl Drop for Search {
    fn drop(&mut self) {
        self.shared.request_stop();
    }
}

/// Start a worker thread whose panic is reported right away, instead of only
/// when the supervisor gets around to joining it.
fn spawn_worker(
    shared: &Arc<Shared>,
    name: String,
    body: impl FnOnce(&Shared) + Send + 'static,
) -> std::io::Result<JoinHandle<()>> {
    struct PanicGuard<'a>(&'a Shared);
    impl Drop for PanicGuard<'_> {
        fn drop(&mut self) {
            if std::thread::panicking() {
                self.0.fail("a search thread crashed unexpectedly".into());
            }
        }
    }

    let shared = Arc::clone(shared);
    std::thread::Builder::new().name(name).spawn(move || {
        let _guard = PanicGuard(&shared);
        body(&shared);
    })
}

/// How often saving a found key is retried after it failed.
const SAVE_RETRY: Duration = Duration::from_secs(10);

/// What became of a candidate passed to [`Shared::submit`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Submitted {
    /// Verified and saved: continue from a fresh random key, because further
    /// keys of the same walk would be related to this one.
    Saved,
    /// Not (or no longer) a wanted match; keep going.
    Rejected,
    /// The search is stopping: return without looking at more candidates.
    Stop,
}
/// State shared by all workers of one search.
pub(crate) struct Shared {
    pub(crate) matcher: Matcher,
    /// Precomputed points for the CPU key walk.
    pub(crate) table: WalkTable,
    /// Start-anchored bit filters, one per pattern.
    pub(crate) filters: Vec<PrefixFilter>,
    /// Addresses wanted per pattern (`u64::MAX` = unlimited).
    target: u64,
    /// Addresses claimed per pattern: saved, or being saved.
    claimed: Vec<AtomicU64>,
    /// Addresses saved per pattern.
    saved: Vec<AtomicU64>,
    completed: Vec<AtomicBool>,
    remaining: AtomicUsize,
    /// Incremented whenever the set of wanted patterns shrinks.
    pub(crate) generation: AtomicU64,
    stop: AtomicBool,
    keys: AtomicU64,
    gpu_keys: AtomicU64,
    near_misses: AtomicU64,
    output_dir: PathBuf,
    started: Instant,
    /// Dropped after [`Event::Finished`], which disconnects the channel.
    events: Mutex<Option<Sender<Event>>>,
}

impl Shared {
    fn new(config: &SearchConfig, events: Sender<Event>) -> Shared {
        let n = config.matcher.patterns().len();
        Shared {
            filters: config
                .matcher
                .patterns()
                .iter()
                .map(|p| p.prefix_filter())
                .collect(),
            matcher: config.matcher.clone(),
            table: WalkTable::new(cpu::GROUP),
            target: config.count.unwrap_or(u64::MAX),
            claimed: (0..n).map(|_| AtomicU64::new(0)).collect(),
            saved: (0..n).map(|_| AtomicU64::new(0)).collect(),
            completed: (0..n).map(|_| AtomicBool::new(false)).collect(),
            remaining: AtomicUsize::new(n),
            generation: AtomicU64::new(0),
            stop: AtomicBool::new(false),
            keys: AtomicU64::new(0),
            gpu_keys: AtomicU64::new(0),
            near_misses: AtomicU64::new(0),
            output_dir: config.output_dir.clone(),
            started: Instant::now(),
            events: Mutex::new(Some(events)),
        }
    }

    pub(crate) fn should_stop(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    pub(crate) fn request_stop(&self) {
        self.stop.store(true, Ordering::Release);
    }

    pub(crate) fn send(&self, event: Event) {
        let events = self.events.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(tx) = events.as_ref() {
            let _ = tx.send(event);
        }
    }

    /// Send [`Event::Finished`] and close the channel.
    fn finish(&self) {
        let mut events = self.events.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(tx) = events.take() {
            let _ = tx.send(Event::Finished);
        }
    }

    /// Sleep for `duration`, waking up early if the search is stopped.
    fn sleep_unless_stopped(&self, duration: Duration) {
        let until = Instant::now() + duration;
        while !self.should_stop() {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            std::thread::sleep(left.min(Duration::from_millis(100)));
        }
    }

    /// Report a fatal error and stop the search.
    pub(crate) fn fail(&self, message: String) {
        self.send(Event::Error(message));
        self.request_stop();
    }

    /// Whether more addresses are wanted for pattern `i`.
    pub(crate) fn wants(&self, i: usize) -> bool {
        self.claimed[i].load(Ordering::Acquire) < self.target
    }

    pub(crate) fn add_keys(&self, n: u64) {
        self.keys.fetch_add(n, Ordering::Relaxed);
    }

    #[cfg(feature = "gpu")]
    pub(crate) fn add_gpu_keys(&self, n: u64) {
        self.keys.fetch_add(n, Ordering::Relaxed);
        self.gpu_keys.fetch_add(n, Ordering::Relaxed);
    }

    pub(crate) fn add_near_miss(&self) {
        self.near_misses.fetch_add(1, Ordering::Relaxed);
    }

    fn stats(&self) -> Stats {
        Stats {
            keys_checked: self.keys.load(Ordering::Relaxed),
            gpu_keys_checked: self.gpu_keys.load(Ordering::Relaxed),
            found: self.saved.iter().map(|f| f.load(Ordering::Relaxed)).sum(),
            near_misses: self.near_misses.load(Ordering::Relaxed),
            elapsed: self.started.elapsed(),
        }
    }

    /// Reserve one of the remaining slots for pattern `i`.
    fn claim(&self, i: usize) -> bool {
        self.claimed[i]
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < self.target).then_some(n + 1)
            })
            .is_ok()
    }

    fn unclaim(&self, i: usize) {
        self.claimed[i].fetch_sub(1, Ordering::AcqRel);
    }

    fn mark_complete_if_done(&self, i: usize) {
        if self.saved[i].load(Ordering::Acquire) >= self.target
            && !self.completed[i].swap(true, Ordering::AcqRel)
        {
            self.generation.fetch_add(1, Ordering::AcqRel);
            if self.remaining.fetch_sub(1, Ordering::AcqRel) == 1 {
                self.request_stop();
            }
        }
    }

    /// Verify and save a candidate key found by a worker. `scalar` is the
    /// secret scalar and `hash_prefix` the secret nonce prefix.
    pub(crate) fn submit(&self, scalar: &[u8; 32], hash_prefix: &[u8; 32]) -> Submitted {
        if self.should_stop() {
            return Submitted::Stop;
        }
        // Keys walked from a clamped start stay clamped unless the 256-bit
        // addition overflowed, which cannot happen in practice.
        if !is_clamped(scalar) {
            return Submitted::Rejected;
        }
        let secret = SecretKey::from_parts(scalar, hash_prefix);
        let pubkey = secret.public_key();
        self.submit_key(&secret, &pubkey)
    }

    /// [`Shared::submit`] for a key whose public key was already derived with
    /// [`SecretKey::public_key`].
    pub(crate) fn submit_key(&self, secret: &SecretKey, pubkey: &[u8; 32]) -> Submitted {
        if self.should_stop() {
            return Submitted::Stop;
        }
        let chars = address_chars(pubkey);
        let mut index = match self.matcher.check(&chars, |i| self.wants(i)) {
            MatchResult::Match(i) => i,
            MatchResult::MissingContains => {
                self.add_near_miss();
                return Submitted::Rejected;
            }
            MatchResult::NoMatch => return Submitted::Rejected,
        };
        if !secret.self_test(pubkey) {
            self.fail(
                "internal error: a generated key failed its self-test; nothing was saved".into(),
            );
            return Submitted::Stop;
        }
        // Another worker may just have taken the last slot of that pattern;
        // the key may still be wanted for another one.
        let mut taken = Vec::new();
        while !self.claim(index) {
            taken.push(index);
            match self
                .matcher
                .check(&chars, |i| self.wants(i) && !taken.contains(&i))
            {
                MatchResult::Match(i) => index = i,
                _ => return Submitted::Rejected,
            }
        }
        self.save(index, secret, pubkey)
    }

    /// Save a key claimed for pattern `index`. If that fails, keep the key and
    /// retry until it works or the search is stopped.
    fn save(&self, index: usize, secret: &SecretKey, pubkey: &[u8; 32]) -> Submitted {
        let address = onion_address(pubkey);
        let mut warned = false;
        loop {
            let error = match write_hidden_service_dir(&self.output_dir, secret) {
                Ok(path) => {
                    self.saved[index].fetch_add(1, Ordering::AcqRel);
                    if warned {
                        self.send(Event::Info(format!("{address} was saved after all.")));
                    }
                    self.send(Event::Found(Found {
                        pattern: self.matcher.patterns()[index].to_string(),
                        address,
                        path,
                    }));
                    self.mark_complete_if_done(index);
                    return Submitted::Saved;
                }
                Err(e) => format!(
                    "could not save the keys for {address} in {}: {e}",
                    self.output_dir.display()
                ),
            };
            if self.should_stop() {
                self.unclaim(index);
                self.fail(format!(
                    "{error}. The search was stopped before the keys could be saved, so this \
                     address is lost."
                ));
                return Submitted::Stop;
            }
            if !warned {
                warned = true;
                self.send(Event::Warning(format!(
                    "{error}. Retrying every {} seconds: fix the problem (for example, free up \
                     disk space) to keep this address. Stopping the search now loses it.",
                    SAVE_RETRY.as_secs()
                )));
            }
            self.sleep_unless_stopped(SAVE_RETRY);
        }
    }
}

/// A random starting key for a walk. Wiped from memory when dropped.
pub(crate) struct Walk {
    pub(crate) start: SecretKey,
}

impl Walk {
    pub(crate) fn random() -> Result<Walk, String> {
        let mut seed = [0u8; 32];
        getrandom::fill(&mut seed)
            .map_err(|e| format!("the operating system's random number generator failed: {e}"))?;
        let start = SecretKey::from_seed(&seed);
        zeroize::Zeroize::zeroize(&mut seed);
        Ok(Walk { start })
    }

    /// The secret scalar of the `k`-th key of this walk: `a + 8k`.
    pub(crate) fn scalar(&self, k: u64) -> [u8; 32] {
        scalar_add(
            self.start.scalar(),
            k.checked_mul(8).expect("walk offset overflow"),
        )
    }
}

/// 256-bit little-endian addition (no modular reduction).
pub(crate) fn scalar_add(base: &[u8; 32], n: u64) -> [u8; 32] {
    let mut out = [0u8; 32];
    let mut carry = u128::from(n);
    for (o, b) in out
        .as_chunks_mut::<8>()
        .0
        .iter_mut()
        .zip(base.as_chunks::<8>().0)
    {
        let sum = u128::from(u64::from_le_bytes(*b)) + carry;
        o.copy_from_slice(&(sum as u64).to_le_bytes());
        carry = sum >> 64;
    }
    out
}

/// Tracks a smoothed key rate from periodic samples.
#[derive(Debug, Clone)]
pub struct RateMeter {
    samples: std::collections::VecDeque<(Instant, u64)>,
    window: Duration,
}

impl RateMeter {
    /// A meter averaging over the given window (e.g. 5 seconds). Create it
    /// when the search starts: it counts from zero keys at this moment.
    pub fn new(window: Duration) -> RateMeter {
        RateMeter {
            samples: [(Instant::now(), 0)].into(),
            window,
        }
    }

    /// Record the current total and return the rate in keys per second.
    pub fn update(&mut self, total_keys: u64) -> f64 {
        let now = Instant::now();
        self.samples.push_back((now, total_keys));
        while self.samples.len() > 2 && now.duration_since(self.samples[0].0) > self.window {
            self.samples.pop_front();
        }
        self.rate()
    }

    /// The current rate in keys per second.
    pub fn rate(&self) -> f64 {
        match (self.samples.front(), self.samples.back()) {
            (Some(&(t0, k0)), Some(&(t1, k1))) if t1 > t0 => {
                (k1.saturating_sub(k0)) as f64 / t1.duration_since(t0).as_secs_f64()
            }
            _ => 0.0,
        }
    }
}

#[cfg(test)]
mod tests;
