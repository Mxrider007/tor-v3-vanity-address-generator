//! CUDA GPU backend for high-speed key generation

use crate::onion::{expanded_secret_key_from_seed, pubkey_to_onion, write_hidden_service_dir};
use crate::pattern::{contains_pattern, is_valid_pattern, matches_prefix};
use crossbeam_channel::{Receiver, Sender};
use std::collections::HashSet;
use std::ffi::CString;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use rustacuda::launch;
use rustacuda::memory::{DeviceBox, DeviceBuffer};
use rustacuda::prelude::*;
use kore_vanity_core as core;

use super::{BackendInfo, FoundKey, GeneratorError, Progress, SearchFilter};

/// CUDA GPU backend for high-speed vanity address generation
#[derive(Debug, Clone)]
pub struct CudaBackend {
    device_count: usize,
    device_names: Vec<String>,
    estimated_speed: u64,
}

impl CudaBackend {
    /// Create a new CUDA backend, detecting available GPUs
    pub fn new() -> Result<Self, GeneratorError> {
        rustacuda::init(CudaFlags::empty())
            .map_err(|e| GeneratorError::Cuda(format!("Failed to initialize CUDA: {}", e)))?;

        let device_count = Device::num_devices()
            .map_err(|e| GeneratorError::Cuda(format!("Failed to count CUDA devices: {}", e)))?
            as usize;

        if device_count == 0 {
            return Err(GeneratorError::Cuda("No CUDA devices found".to_string()));
        }

        let mut device_names = Vec::new();
        let mut total_speed = 0u64;

        for i in 0..device_count {
            let device = Device::get_device(i as u32)
                .map_err(|e| GeneratorError::Cuda(format!("Failed to get device {}: {}", i, e)))?;

            let name = device.name()
                .map_err(|e| GeneratorError::Cuda(format!("Failed to get device name: {}", e)))?;

            // Estimate speed based on multiprocessor count
            let mp_count = device
                .get_attribute(rustacuda::device::DeviceAttribute::MultiprocessorCount)
                .unwrap_or(1) as u64;

            // Rough estimate: ~5M keys/sec per SM for GTX 10xx series
            let device_speed = mp_count * 5_000_000;
            total_speed += device_speed;

            device_names.push(name);
        }

        Ok(Self {
            device_count,
            device_names,
            estimated_speed: total_speed,
        })
    }

    /// Get backend information
    pub fn info(&self) -> BackendInfo {
        let name = if self.device_count == 1 {
            format!("CUDA ({})", self.device_names[0])
        } else {
            format!("CUDA ({} GPUs: {})", self.device_count, self.device_names.join(", "))
        };

        BackendInfo {
            name,
            estimated_speed: self.estimated_speed,
        }
    }

    /// Start vanity address generation on GPU
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

    /// Start GPU generation, applying the `contains` post-filter on each hit.
    ///
    /// The GPU kernel can only match a literal byte-prefix at the start of the
    /// address, so `?` wildcards and `--anywhere` are rejected here (use the CPU
    /// or hybrid backend for those). The `contains` words are applied host-side
    /// after each GPU match.
    pub fn generate_with_filter(
        &self,
        prefixes: Vec<String>,
        output_dir: PathBuf,
        progress_tx: Sender<Progress>,
        result_tx: Sender<FoundKey>,
        stop_rx: Receiver<()>,
        filter: SearchFilter,
    ) -> Result<(), GeneratorError> {
        // Validate prefixes. The GPU kernel matches a literal byte-prefix, so it
        // cannot honor `?` wildcards or `--anywhere`.
        if filter.prefix_anywhere {
            return Err(GeneratorError::Cuda(
                "the built-in CUDA backend cannot match patterns 'anywhere'; \
                 use --mode cpu or --mode hybrid"
                    .to_string(),
            ));
        }
        for prefix in &prefixes {
            if !is_valid_pattern(prefix) {
                return Err(GeneratorError::InvalidPrefix(prefix.clone()));
            }
            if prefix.contains('?') {
                return Err(GeneratorError::Cuda(format!(
                    "the built-in CUDA backend cannot match the '?' wildcard in '{}'; \
                     use --mode cpu or --mode hybrid",
                    prefix
                )));
            }
        }

        // Shared state
        let remaining: Arc<Mutex<HashSet<String>>> =
            Arc::new(Mutex::new(prefixes.clone().into_iter().collect()));
        let counter = Arc::new(AtomicU64::new(0));
        let prefix_match_counter = Arc::new(AtomicU64::new(0));
        let contains_reject_counter = Arc::new(AtomicU64::new(0));
        let stopped = Arc::new(AtomicBool::new(false));
        let start_time = Instant::now();
        let contains: Arc<Vec<String>> = Arc::new(filter.contains.clone());

        // Spawn a thread for each GPU
        let mut handles = Vec::new();
        let gpu_error: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

        for device_idx in 0..self.device_count {
            let prefixes = prefixes.clone();
            let output_dir = output_dir.clone();
            let result_tx = result_tx.clone();
            let remaining = remaining.clone();
            let counter = counter.clone();
            let stopped = stopped.clone();
            let gpu_error = gpu_error.clone();
            let contains = contains.clone();
            let prefix_match_counter = prefix_match_counter.clone();
            let contains_reject_counter = contains_reject_counter.clone();

            let handle = std::thread::spawn(move || {
                if let Err(e) = Self::gpu_worker(
                    device_idx as u32,
                    prefixes,
                    output_dir,
                    result_tx,
                    remaining,
                    counter,
                    stopped.clone(),
                    contains,
                    prefix_match_counter,
                    contains_reject_counter,
                ) {
                    eprintln!("GPU {} error: {}", device_idx, e);
                    *gpu_error.lock().unwrap() = Some(format!("GPU {} error: {}", device_idx, e));
                    stopped.store(true, Ordering::SeqCst);
                }
            });

            handles.push(handle);
        }

        // Progress reporting thread
        let progress_stopped = stopped.clone();
        let progress_counter = counter.clone();
        let progress_remaining = remaining.clone();
        let progress_prefix_counter = prefix_match_counter.clone();
        let progress_reject_counter = contains_reject_counter.clone();
        let progress_handle = std::thread::spawn(move || {
            while !progress_stopped.load(Ordering::Relaxed) {
                let keys_checked = progress_counter.load(Ordering::Relaxed);
                let elapsed = start_time.elapsed().as_secs_f64();
                let keys_per_sec = if elapsed > 0.0 {
                    keys_checked as f64 / elapsed
                } else {
                    0.0
                };

                let _ = progress_tx.send(Progress {
                    keys_checked,
                    keys_per_sec,
                    elapsed_secs: elapsed,
                    prefix_matches: progress_prefix_counter.load(Ordering::Relaxed),
                    contains_rejects: progress_reject_counter.load(Ordering::Relaxed),
                });

                // Check if done
                if progress_remaining.lock().unwrap().is_empty() {
                    break;
                }

                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        });

        // Stop signal handler
        let stop_stopped = stopped.clone();
        std::thread::spawn(move || {
            if stop_rx.recv().is_ok() {
                stop_stopped.store(true, Ordering::SeqCst);
            }
        });

        // Wait for completion or all prefixes found
        loop {
            if remaining.lock().unwrap().is_empty() || stopped.load(Ordering::SeqCst) {
                stopped.store(true, Ordering::SeqCst);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }

        // Wait for all threads
        for handle in handles {
            let _ = handle.join();
        }
        let _ = progress_handle.join();

        // Check for GPU errors
        if let Some(err) = gpu_error.lock().unwrap().take() {
            return Err(GeneratorError::Cuda(err));
        }

        if stopped.load(Ordering::SeqCst) && !remaining.lock().unwrap().is_empty() {
            Err(GeneratorError::Stopped)
        } else {
            Ok(())
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn gpu_worker(
        device_idx: u32,
        prefixes: Vec<String>,
        output_dir: PathBuf,
        result_tx: Sender<FoundKey>,
        remaining: Arc<Mutex<HashSet<String>>>,
        counter: Arc<AtomicU64>,
        stopped: Arc<AtomicBool>,
        contains: Arc<Vec<String>>,
        prefix_match_counter: Arc<AtomicU64>,
        contains_reject_counter: Arc<AtomicU64>,
    ) -> Result<(), GeneratorError> {
        use rand::RngCore;

        let device = Device::get_device(device_idx)
            .map_err(|e| GeneratorError::Cuda(format!("Failed to get device: {}", e)))?;

        let _context = Context::create_and_push(
            ContextFlags::MAP_HOST | ContextFlags::SCHED_AUTO,
            device,
        )
        .map_err(|e| GeneratorError::Cuda(format!("Failed to create context: {}", e)))?;

        // Load PTX module
        let module_data = CString::new(include_str!(env!("KERNEL_PTX_PATH")))
            .map_err(|e| GeneratorError::Cuda(format!("Invalid PTX: {}", e)))?;

        let kernel = Module::load_from_string(&module_data)
            .map_err(|e| GeneratorError::Cuda(format!("Failed to load module: {}", e)))?;

        let function = kernel
            .get_function(c"render")
            .map_err(|e| GeneratorError::Cuda(format!("Failed to get function: {}", e)))?;

        let stream = Stream::new(StreamFlags::NON_BLOCKING, None)
            .map_err(|e| GeneratorError::Cuda(format!("Failed to create stream: {}", e)))?;

        // Set up GPU memory
        let mut seed = [0u8; 32];
        let mut gpu_seed = DeviceBuffer::from_slice(&seed)
            .map_err(|e| GeneratorError::Cuda(format!("Failed to allocate seed buffer: {}", e)))?;

        let mut byte_prefixes_owned: Vec<_> = prefixes
            .iter()
            .map(|s| BytePrefixOwned::from_str(s))
            .collect();

        let mut byte_prefixes: Vec<_> = byte_prefixes_owned
            .iter_mut()
            .map(|bp| bp.as_byte_prefix())
            .collect();

        let mut gpu_byte_prefixes = DeviceBuffer::from_slice(&byte_prefixes)
            .map_err(|e| GeneratorError::Cuda(format!("Failed to allocate prefix buffer: {}", e)))?;

        let mut params = DeviceBox::new(&core::KernelParams {
            seed: gpu_seed.as_device_ptr(),
            byte_prefixes: gpu_byte_prefixes.as_device_ptr(),
            byte_prefixes_len: gpu_byte_prefixes.len(),
        })
        .map_err(|e| GeneratorError::Cuda(format!("Failed to allocate params: {}", e)))?;

        // Calculate optimal thread/block configuration
        let fn_max_threads = function
            .get_attribute(rustacuda::function::FunctionAttribute::MaxThreadsPerBlock)
            .unwrap_or(256) as u32;

        let fn_registers = function
            .get_attribute(rustacuda::function::FunctionAttribute::NumRegisters)
            .unwrap_or(32) as u32;

        let gpu_max_threads = device
            .get_attribute(rustacuda::device::DeviceAttribute::MaxThreadsPerBlock)
            .unwrap_or(1024) as u32;

        let gpu_max_registers = device
            .get_attribute(rustacuda::device::DeviceAttribute::MaxRegistersPerBlock)
            .unwrap_or(65536) as u32;

        let gpu_cores = device
            .get_attribute(rustacuda::device::DeviceAttribute::MultiprocessorCount)
            .unwrap_or(1) as u32;

        let threads = *[
            fn_max_threads,
            gpu_max_threads,
            gpu_max_registers.checked_div(fn_registers).unwrap_or(256),
        ]
        .iter()
        .min()
        .unwrap();

        let blocks = gpu_cores * gpu_max_threads / threads;

        let mut rng = rand::thread_rng();

        // Main generation loop
        while !stopped.load(Ordering::Relaxed) {
            // Check if all prefixes found
            if remaining.lock().unwrap().is_empty() {
                break;
            }

            // Generate new random seed
            rng.fill_bytes(&mut seed);
            gpu_seed.copy_from(&seed)
                .map_err(|e| GeneratorError::Cuda(format!("Failed to copy seed: {}", e)))?;

            // Launch kernel
            unsafe {
                launch!(kernel.render<<<blocks, threads, 0, stream>>>(params.as_device_ptr()))
                    .map_err(|e| GeneratorError::Cuda(format!("Kernel launch failed: {}", e)))?;
            }

            stream.synchronize()
                .map_err(|e| GeneratorError::Cuda(format!("Stream sync failed: {}", e)))?;

            // Check results
            gpu_byte_prefixes.copy_to(&mut byte_prefixes)
                .map_err(|e| GeneratorError::Cuda(format!("Failed to copy results: {}", e)))?;

            for (i, prefix) in byte_prefixes_owned.iter_mut().enumerate() {
                let mut success = false;
                prefix.success.copy_to(&mut success).ok();

                if success {
                    prefix.success.copy_from(&false).ok();

                    // The PTX kernel writes the matching SEED into `out`.
                    let mut seed = [0u8; 32];
                    prefix.out.copy_to(&mut seed).ok();

                    // Derive the public key and onion from the seed (standard
                    // ed25519 seed expansion).
                    let signing_key = ed25519_dalek::SigningKey::from_bytes(&seed);
                    let pubkey_bytes = signing_key.verifying_key().to_bytes();
                    let onion = pubkey_to_onion(&pubkey_bytes);

                    let prefix_str = prefixes[i].clone();

                    // The kernel matches on raw pubkey bytes with a last-byte
                    // mask, which can diverge from a base32-char prefix at the
                    // boundary. Re-validate against the real address to reject
                    // false positives.
                    if !matches_prefix(&onion, &prefix_str) {
                        continue;
                    }
                    prefix_match_counter.fetch_add(1, Ordering::Relaxed);

                    // Apply the host-side `contains` filter.
                    if !contains.iter().all(|w| contains_pattern(&onion, w)) {
                        contains_reject_counter.fetch_add(1, Ordering::Relaxed);
                        continue;
                    }

                    // Claim the prefix; skip if another GPU already took it.
                    if !remaining.lock().unwrap().remove(&prefix_str) {
                        continue;
                    }

                    // Write the proper Tor hidden-service directory (derives the
                    // clamped expanded secret key from the seed).
                    let expanded = expanded_secret_key_from_seed(&seed);
                    if let Ok(hs_dir) =
                        write_hidden_service_dir(&output_dir, &onion, &pubkey_bytes, &expanded)
                    {
                        let _ = result_tx.send(FoundKey {
                            prefix: prefix_str,
                            onion_address: onion,
                            key_path: hs_dir,
                        });
                    }
                }
            }

            counter.fetch_add((threads * blocks) as u64, Ordering::Relaxed);
        }

        Ok(())
    }
}

/// GPU-side prefix matching structure
struct BytePrefixOwned {
    byte_prefix: DeviceBuffer<u8>,
    last_byte_idx: usize,
    last_byte_mask: u8,
    out: DeviceBuffer<u8>,
    success: DeviceBox<bool>,
}

impl BytePrefixOwned {
    fn from_str(s: &str) -> Self {
        // Onion addresses are lowercase base32; match the kernel byte-prefix
        // against the lowercased form. Validated (a-z, 2-7, no '?') by the caller.
        let byte_prefix = base32::decode(
            base32::Alphabet::Rfc4648Lower { padding: false },
            &format!("{}aa", s.to_lowercase()),
        )
        .expect("prefix must be valid base32");

        let mut last_byte_idx = (5 * s.len() / 8).saturating_sub(1);

        let n_bits = (5 * s.len()) % 8;
        let last_byte_mask = ((1u16 << n_bits) - 1) as u8;
        let last_byte_mask = last_byte_mask << (8 - n_bits);

        if last_byte_mask > 0 && last_byte_idx < byte_prefix.len().saturating_sub(1) {
            last_byte_idx += 1;
        }

        let gpu_byte_prefix = DeviceBuffer::from_slice(&byte_prefix).unwrap();
        let out = [0u8; 32];
        let gpu_out = DeviceBuffer::from_slice(&out).unwrap();
        let success = false;
        let gpu_success = DeviceBox::new(&success).unwrap();

        BytePrefixOwned {
            byte_prefix: gpu_byte_prefix,
            last_byte_idx,
            last_byte_mask,
            out: gpu_out,
            success: gpu_success,
        }
    }

    fn as_byte_prefix(&mut self) -> core::BytePrefix {
        core::BytePrefix {
            byte_prefix: self.byte_prefix.as_device_ptr(),
            byte_prefix_len: self.byte_prefix.len(),
            last_byte_idx: self.last_byte_idx,
            last_byte_mask: self.last_byte_mask,
            out: self.out.as_device_ptr(),
            success: self.success.as_device_ptr(),
        }
    }
}
