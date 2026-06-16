//! Hybrid CPU+GPU backend for maximum throughput
//!
//! Runs both the CPU and the built-in CUDA backends simultaneously, combining
//! their speeds. They share a single `remaining` prefix set so each prefix is
//! reported exactly once, regardless of which side finds it first.

use crate::onion::{expanded_secret_key_from_seed, pubkey_to_onion, write_hidden_service_dir};
use crate::pattern::{contains_pattern, is_valid_pattern, matches_prefix};
use crossbeam_channel::{unbounded, Receiver, Sender};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use super::cpu::CpuBackend;
use super::cuda::CudaBackend;
use super::{BackendInfo, FoundKey, GeneratorError, Progress, SearchFilter};

/// Hybrid backend that runs CPU and GPU in parallel
#[derive(Debug, Clone)]
pub struct HybridBackend {
    cpu_threads: usize,
    gpu_info: String,
    cpu_speed: u64,
    gpu_speed: u64,
}

impl HybridBackend {
    /// Create a new hybrid backend with default settings
    pub fn new() -> Result<Self, GeneratorError> {
        Self::with_cpu_threads(num_cpus::get())
    }

    /// Create a hybrid backend with specified CPU thread count
    pub fn with_cpu_threads(cpu_threads: usize) -> Result<Self, GeneratorError> {
        // Verify CUDA is available
        let cuda = CudaBackend::new()?;
        let cuda_info = cuda.info();

        let cpu = CpuBackend::with_threads(cpu_threads);
        let cpu_info = cpu.info();

        Ok(Self {
            cpu_threads,
            gpu_info: cuda_info.name,
            cpu_speed: cpu_info.estimated_speed,
            gpu_speed: cuda_info.estimated_speed,
        })
    }

    /// Get backend information
    pub fn info(&self) -> BackendInfo {
        BackendInfo {
            name: format!(
                "Hybrid: {} + CPU ({} threads)",
                self.gpu_info, self.cpu_threads
            ),
            estimated_speed: self.cpu_speed + self.gpu_speed,
        }
    }

    /// Start generation on both CPU and GPU simultaneously
    pub fn generate(
        &self,
        prefixes: Vec<String>,
        output_dir: PathBuf,
        progress_tx: Sender<Progress>,
        result_tx: Sender<FoundKey>,
        stop_rx: Receiver<()>,
    ) -> Result<(), GeneratorError> {
        self.generate_with_filter(
            prefixes, output_dir, progress_tx, result_tx, stop_rx, SearchFilter::default(),
        )
    }

    /// Start generation on both CPU and GPU, honoring the search filter.
    ///
    /// The CPU half handles the full pattern language (`?` wildcards, `contains`,
    /// `anywhere`). The GPU half only accelerates literal start-anchored prefixes,
    /// so it is given the subset of prefixes without wildcards (and is skipped
    /// entirely when `anywhere` is set or every prefix uses a wildcard).
    pub fn generate_with_filter(
        &self,
        prefixes: Vec<String>,
        output_dir: PathBuf,
        progress_tx: Sender<Progress>,
        result_tx: Sender<FoundKey>,
        stop_rx: Receiver<()>,
        filter: SearchFilter,
    ) -> Result<(), GeneratorError> {
        // Validate prefixes (allows a-z, 2-7, ? as wildcard)
        for prefix in &prefixes {
            if !is_valid_pattern(prefix) {
                return Err(GeneratorError::InvalidPrefix(prefix.clone()));
            }
        }

        // Shared state across CPU and GPU
        let remaining: Arc<Mutex<HashSet<String>>> =
            Arc::new(Mutex::new(prefixes.clone().into_iter().collect()));
        let cpu_counter = Arc::new(AtomicU64::new(0));
        let gpu_counter = Arc::new(AtomicU64::new(0));
        let prefix_match_counter = Arc::new(AtomicU64::new(0));
        let contains_reject_counter = Arc::new(AtomicU64::new(0));
        let stopped = Arc::new(AtomicBool::new(false));
        let start_time = Instant::now();

        // Spawn CPU worker threads (handle the full pattern language).
        let cpu_handles = self.spawn_cpu_workers(
            output_dir.clone(),
            result_tx.clone(),
            remaining.clone(),
            cpu_counter.clone(),
            prefix_match_counter.clone(),
            contains_reject_counter.clone(),
            stopped.clone(),
            filter.clone(),
        );

        // Spawn the GPU half (literal prefixes only).
        let gpu_handle = Self::spawn_gpu_workers(
            prefixes,
            output_dir,
            result_tx.clone(),
            remaining.clone(),
            gpu_counter.clone(),
            stopped.clone(),
            filter,
        );

        // Progress reporting
        let progress_stopped = stopped.clone();
        let progress_cpu_counter = cpu_counter.clone();
        let progress_gpu_counter = gpu_counter.clone();
        let progress_prefix_counter = prefix_match_counter.clone();
        let progress_reject_counter = contains_reject_counter.clone();
        let progress_remaining = remaining.clone();

        let progress_handle = std::thread::spawn(move || {
            while !progress_stopped.load(Ordering::Relaxed) {
                let cpu_keys = progress_cpu_counter.load(Ordering::Relaxed);
                let gpu_keys = progress_gpu_counter.load(Ordering::Relaxed);
                let total_keys = cpu_keys + gpu_keys;

                let elapsed = start_time.elapsed().as_secs_f64();
                let keys_per_sec = if elapsed > 0.0 {
                    total_keys as f64 / elapsed
                } else {
                    0.0
                };

                let _ = progress_tx.send(Progress {
                    keys_checked: total_keys,
                    keys_per_sec,
                    elapsed_secs: elapsed,
                    prefix_matches: progress_prefix_counter.load(Ordering::Relaxed),
                    contains_rejects: progress_reject_counter.load(Ordering::Relaxed),
                });

                if progress_remaining.lock().unwrap().is_empty() {
                    break;
                }

                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        });

        // Handle stop signal from the caller.
        let stop_stopped = stopped.clone();
        std::thread::spawn(move || {
            if stop_rx.recv().is_ok() {
                stop_stopped.store(true, Ordering::SeqCst);
            }
        });

        // Wait for completion (all prefixes found, or stopped).
        loop {
            if remaining.lock().unwrap().is_empty() || stopped.load(Ordering::SeqCst) {
                stopped.store(true, Ordering::SeqCst);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        // Wait for all workers
        for handle in cpu_handles {
            let _ = handle.join();
        }
        if let Some(handle) = gpu_handle {
            let _ = handle.join();
        }
        let _ = progress_handle.join();

        if stopped.load(Ordering::SeqCst) && !remaining.lock().unwrap().is_empty() {
            Err(GeneratorError::Stopped)
        } else {
            Ok(())
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn spawn_cpu_workers(
        &self,
        output_dir: PathBuf,
        result_tx: Sender<FoundKey>,
        remaining: Arc<Mutex<HashSet<String>>>,
        counter: Arc<AtomicU64>,
        prefix_match_counter: Arc<AtomicU64>,
        contains_reject_counter: Arc<AtomicU64>,
        stopped: Arc<AtomicBool>,
        filter: SearchFilter,
    ) -> Vec<std::thread::JoinHandle<()>> {
        let mut handles = Vec::new();

        for _ in 0..self.cpu_threads {
            let output_dir = output_dir.clone();
            let result_tx = result_tx.clone();
            let remaining = remaining.clone();
            let counter = counter.clone();
            let prefix_match_counter = prefix_match_counter.clone();
            let contains_reject_counter = contains_reject_counter.clone();
            let stopped = stopped.clone();
            let filter = filter.clone();

            let handle = std::thread::spawn(move || {
                Self::cpu_worker(
                    output_dir,
                    result_tx,
                    remaining,
                    counter,
                    prefix_match_counter,
                    contains_reject_counter,
                    stopped,
                    filter,
                );
            });

            handles.push(handle);
        }

        handles
    }

    #[allow(clippy::too_many_arguments)]
    fn cpu_worker(
        output_dir: PathBuf,
        result_tx: Sender<FoundKey>,
        remaining: Arc<Mutex<HashSet<String>>>,
        counter: Arc<AtomicU64>,
        prefix_match_counter: Arc<AtomicU64>,
        contains_reject_counter: Arc<AtomicU64>,
        stopped: Arc<AtomicBool>,
        filter: SearchFilter,
    ) {
        while !stopped.load(Ordering::Relaxed) {
            if remaining.lock().unwrap().is_empty() {
                break;
            }

            // Generate random seed and keypair.
            let seed: [u8; 32] = rand::random();
            let signing_key = ed25519_dalek::SigningKey::from_bytes(&seed);
            let pubkey_bytes: [u8; 32] = signing_key.verifying_key().to_bytes();
            let onion = pubkey_to_onion(&pubkey_bytes);

            // Match against remaining prefixes with the full pattern language.
            let remaining_guard = remaining.lock().unwrap();
            let mut found_prefix = None;
            for prefix in remaining_guard.iter() {
                let matches = if filter.prefix_anywhere {
                    contains_pattern(&onion, prefix)
                } else {
                    matches_prefix(&onion, prefix)
                };
                if matches {
                    found_prefix = Some(prefix.clone());
                    break;
                }
            }
            drop(remaining_guard);

            if let Some(prefix) = found_prefix {
                prefix_match_counter.fetch_add(1, Ordering::Relaxed);

                // Apply the contains filter.
                if !filter.contains.iter().all(|w| contains_pattern(&onion, w)) {
                    contains_reject_counter.fetch_add(1, Ordering::Relaxed);
                    counter.fetch_add(1, Ordering::Relaxed);
                    continue;
                }

                // Claim the prefix; only the first to claim writes/reports it.
                if remaining.lock().unwrap().remove(&prefix) {
                    let expanded = expanded_secret_key_from_seed(&seed);
                    if let Ok(hs_dir) =
                        write_hidden_service_dir(&output_dir, &onion, &pubkey_bytes, &expanded)
                    {
                        let _ = result_tx.send(FoundKey {
                            prefix,
                            onion_address: onion,
                            key_path: hs_dir,
                        });
                    }
                }
            }

            counter.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn spawn_gpu_workers(
        prefixes: Vec<String>,
        output_dir: PathBuf,
        result_tx: Sender<FoundKey>,
        remaining: Arc<Mutex<HashSet<String>>>,
        counter: Arc<AtomicU64>,
        stopped: Arc<AtomicBool>,
        filter: SearchFilter,
    ) -> Option<std::thread::JoinHandle<()>> {
        // The GPU kernel only matches literal start-anchored prefixes. Skip it
        // when matching anywhere; otherwise give it the wildcard-free prefixes.
        let gpu_prefixes: Vec<String> = if filter.prefix_anywhere {
            Vec::new()
        } else {
            prefixes.into_iter().filter(|p| !p.contains('?')).collect()
        };
        if gpu_prefixes.is_empty() {
            return None;
        }

        let handle = std::thread::spawn(move || {
            let cuda = match CudaBackend::new() {
                Ok(c) => c,
                Err(_) => return,
            };

            // Internal channels so we can bridge GPU output into shared state.
            let (internal_progress_tx, internal_progress_rx) = unbounded::<Progress>();
            let (internal_result_tx, internal_result_rx) = unbounded::<FoundKey>();
            let (internal_stop_tx, internal_stop_rx) = crossbeam_channel::bounded(1);

            // Forward GPU progress into the shared GPU counter.
            let progress_counter = counter.clone();
            std::thread::spawn(move || {
                while let Ok(p) = internal_progress_rx.recv() {
                    progress_counter.store(p.keys_checked, Ordering::Relaxed);
                }
            });

            // Bridge GPU results: claim in the shared set so each prefix is
            // reported once across CPU and GPU.
            let bridge_remaining = remaining.clone();
            let bridge_result_tx = result_tx;
            std::thread::spawn(move || {
                while let Ok(found) = internal_result_rx.recv() {
                    if bridge_remaining.lock().unwrap().remove(&found.prefix) {
                        let _ = bridge_result_tx.send(found);
                    }
                }
            });

            // Stop the GPU when the shared state says we're done.
            let monitor_stopped = stopped.clone();
            let monitor_remaining = remaining.clone();
            std::thread::spawn(move || {
                loop {
                    if monitor_stopped.load(Ordering::Relaxed)
                        || monitor_remaining.lock().unwrap().is_empty()
                    {
                        let _ = internal_stop_tx.send(());
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            });

            let _ = cuda.generate_with_filter(
                gpu_prefixes,
                output_dir,
                internal_progress_tx,
                internal_result_tx,
                internal_stop_rx,
                filter,
            );
        });

        Some(handle)
    }
}
