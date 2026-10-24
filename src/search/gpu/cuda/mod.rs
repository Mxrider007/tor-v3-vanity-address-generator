//! CUDA backend for NVIDIA graphics cards: the kernel in `kernel.cu` (the
//! same search as kernel.wgsl), run through the NVIDIA driver.
//!
//! The PTX is embedded in the program and compiled for the card by the
//! driver when the search starts; the driver caches the result. Like the
//! wgpu backend, the device must pass [`super::self_test`] before it is used,
//! and every candidate it reports is verified on the CPU.

mod driver;
#[cfg(all(test, feature = "kernel-test"))]
pub(super) mod host;

use std::cell::Cell;
use std::ffi::{c_char, c_int, c_void};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use driver::{CuContext, CuDevice, CuDevicePtr, CuFunction, CuModule, Driver};

use super::{Device, Hit, MAX_HITS, POLL_TIMEOUT, SELF_TEST_TARGETS, WORKGROUP_SIZE};

/// Generated from kernel.cu by scripts/build-cuda-kernel.sh.
const PTX: &str = concat!(include_str!("kernel.ptx"), "\0");

/// The PTX targets compute capability 5.0 (GeForce GTX 750 and newer).
const MIN_COMPUTE_CAPABILITY: (c_int, c_int) = (5, 0);

/// An NVIDIA GPU found through CUDA.
#[derive(Debug, Clone)]
pub(super) struct CudaDevice {
    ordinal: c_int,
    pub(super) name: String,
    pub(super) integrated: bool,
    multiprocessors: u32,
}

/// The NVIDIA GPUs that can run the kernel, or why CUDA cannot be used at
/// all (for example because there is no NVIDIA driver).
pub(super) fn status() -> Result<Vec<CudaDevice>, String> {
    let d = driver::driver()?;
    let mut count = 0;
    // SAFETY: valid out pointer.
    d.check("cuDeviceGetCount", unsafe {
        (d.device_get_count)(&mut count)
    })?;
    Ok((0..count)
        .filter_map(|ordinal| device_info(d, ordinal).ok())
        .filter(|info| info.1 >= MIN_COMPUTE_CAPABILITY)
        .map(|(device, _)| device)
        .collect())
}

/// The NVIDIA GPUs that can run the kernel (none without an NVIDIA driver).
pub(super) fn devices() -> Vec<CudaDevice> {
    status().unwrap_or_default()
}

/// GPUs whose kernel did not finish in time. A stuck kernel keeps the GPU's
/// CUDA context busy for the rest of the process, so they are not used again.
static STUCK: Mutex<Vec<c_int>> = Mutex::new(Vec::new());

fn is_stuck(ordinal: c_int) -> bool {
    STUCK.lock().map_or(true, |stuck| stuck.contains(&ordinal))
}

fn device_info(d: &Driver, ordinal: c_int) -> Result<(CudaDevice, (c_int, c_int)), String> {
    let mut device: CuDevice = 0;
    // SAFETY: valid out pointer; the ordinal is below the device count.
    d.check("cuDeviceGet", unsafe {
        (d.device_get)(&mut device, ordinal)
    })?;
    let attribute = |attribute| {
        let mut value = 0;
        // SAFETY: valid out pointer and device.
        d.check("cuDeviceGetAttribute", unsafe {
            (d.device_get_attribute)(&mut value, attribute, device)
        })
        .map(|()| value)
    };
    let mut name = [0 as c_char; 256];
    // SAFETY: the driver writes at most `len` bytes, NUL-terminated.
    d.check("cuDeviceGetName", unsafe {
        (d.device_get_name)(name.as_mut_ptr(), name.len() as c_int, device)
    })?;
    // SAFETY: NUL-terminated by the driver (and the buffer ends in zeros).
    let name = unsafe { std::ffi::CStr::from_ptr(name.as_ptr()) }
        .to_string_lossy()
        .trim()
        .to_string();
    let capability = (
        attribute(driver::ATTRIBUTE_COMPUTE_CAPABILITY_MAJOR)?,
        attribute(driver::ATTRIBUTE_COMPUTE_CAPABILITY_MINOR)?,
    );
    Ok((
        CudaDevice {
            ordinal,
            name,
            integrated: attribute(driver::ATTRIBUTE_INTEGRATED)? != 0,
            multiprocessors: attribute(driver::ATTRIBUTE_MULTIPROCESSOR_COUNT)?.max(1) as u32,
        },
        capability,
    ))
}

/// A device buffer.
struct Buffer {
    ptr: CuDevicePtr,
    size: usize,
}

/// An NVIDIA GPU ready to run the kernel. Must be used and dropped on the
/// thread that created it (the CUDA context is current there).
pub(super) struct CudaGpu {
    d: &'static Driver,
    ordinal: c_int,
    device: CuDevice,
    context: CuContext,
    module: CuModule,
    function: CuFunction,
    buffers: Vec<Buffer>,
    max_threads: u32,
    /// A dispatch timed out, so the kernel may still be running.
    busy: Cell<bool>,
}

// Indices into `CudaGpu::buffers`.
const TABLE: usize = 0;
const FILTERS: usize = 1;
const CENTERS: usize = 2;
const HITS: usize = 3;

const HITS_WORDS: usize = 1 + 3 * MAX_HITS;

impl CudaGpu {
    pub(super) fn new(info: &CudaDevice, max_filters: usize) -> Result<CudaGpu, String> {
        if is_stuck(info.ordinal) {
            return Err(
                "it stopped responding earlier; restart the program to use it again".into(),
            );
        }
        let d = driver::driver()?;
        let mut device: CuDevice = 0;
        // SAFETY: valid out pointer.
        d.check("cuDeviceGet", unsafe {
            (d.device_get)(&mut device, info.ordinal)
        })?;
        let mut context: CuContext = std::ptr::null_mut();
        // SAFETY: valid out pointer and device.
        d.check("cuDevicePrimaryCtxRetain", unsafe {
            (d.primary_ctx_retain)(&mut context, device)
        })?;
        // From here on, Drop releases what has been set up.
        let mut gpu = CudaGpu {
            d,
            ordinal: info.ordinal,
            device,
            context,
            module: std::ptr::null_mut(),
            function: std::ptr::null_mut(),
            buffers: Vec::new(),
            // A few waves of threads per multiprocessor.
            max_threads: (info.multiprocessors * 1024).clamp(4096, 262_144),
            busy: Cell::new(false),
        };
        gpu.make_current()?;
        gpu.load_module()?;

        let table = super::table_words_packed();
        for words in [
            table.len(),
            max_filters.max(SELF_TEST_TARGETS) * 16,
            gpu.max_threads as usize * 16,
            HITS_WORDS,
        ] {
            let size = words * 4;
            let mut ptr: CuDevicePtr = 0;
            // SAFETY: valid out pointer; the context is current.
            d.check("cuMemAlloc", unsafe { (d.mem_alloc)(&mut ptr, size) })?;
            gpu.buffers.push(Buffer { ptr, size });
        }
        gpu.write(TABLE, &table)?;
        Ok(gpu)
    }

    fn make_current(&self) -> Result<(), String> {
        // SAFETY: the context is retained until Drop.
        self.d.check("cuCtxSetCurrent", unsafe {
            (self.d.ctx_set_current)(self.context)
        })
    }

    /// Have the driver compile the PTX for this GPU.
    fn load_module(&mut self) -> Result<(), String> {
        let mut log = vec![0u8; 8192];
        let mut options = [
            driver::JIT_ERROR_LOG_BUFFER,
            driver::JIT_ERROR_LOG_BUFFER_SIZE_BYTES,
        ];
        let mut values = [
            log.as_mut_ptr().cast::<c_void>(),
            std::ptr::without_provenance_mut(log.len()),
        ];
        // SAFETY: the image is NUL-terminated PTX; the options point to a
        // writable log buffer of the given size.
        let result = unsafe {
            (self.d.module_load_data_ex)(
                &mut self.module,
                PTX.as_ptr().cast(),
                options.len() as u32,
                options.as_mut_ptr(),
                values.as_mut_ptr(),
            )
        };
        if let Err(e) = self.d.check("cuModuleLoadDataEx", result) {
            let log = String::from_utf8_lossy(&log);
            let log = log.trim_end_matches('\0').trim();
            return Err(if log.is_empty() {
                format!("the driver could not compile the GPU program: {e}")
            } else {
                format!("the driver could not compile the GPU program: {e}: {log}")
            });
        }
        // SAFETY: valid out pointer, loaded module and NUL-terminated name.
        self.d.check("cuModuleGetFunction", unsafe {
            (self.d.module_get_function)(&mut self.function, self.module, c"search".as_ptr())
        })
    }

    fn write(&self, buffer: usize, words: &[u32]) -> Result<(), String> {
        let b = &self.buffers[buffer];
        let size = std::mem::size_of_val(words);
        if size == 0 {
            return Ok(());
        }
        if size > b.size {
            return Err("internal error: GPU buffer too small".into());
        }
        // SAFETY: the source holds `size` bytes and the buffer is at least
        // that large.
        self.d.check("cuMemcpyHtoD", unsafe {
            (self.d.memcpy_htod)(b.ptr, words.as_ptr().cast(), size)
        })
    }

    fn read(&self, buffer: usize, words: usize) -> Result<Vec<u32>, String> {
        let b = &self.buffers[buffer];
        let mut out = vec![0u32; words];
        let size = words * 4;
        if size > b.size {
            return Err("internal error: GPU buffer too small".into());
        }
        // SAFETY: the destination holds `size` bytes and the buffer is at
        // least that large.
        self.d.check("cuMemcpyDtoH", unsafe {
            (self.d.memcpy_dtoh)(out.as_mut_ptr().cast(), b.ptr, size)
        })?;
        Ok(out)
    }
}

impl Device for CudaGpu {
    fn max_threads(&self) -> u32 {
        self.max_threads
    }

    fn upload_centers(&self, words: &[u32]) -> Result<(), String> {
        self.write(CENTERS, words)
    }

    fn upload_filters(&self, filters: &[[u32; 16]]) -> Result<(), String> {
        self.write(FILTERS, filters.as_flattened())
    }

    fn dispatch(
        &self,
        groups: u32,
        threads: u32,
        filters: u32,
        words: u32,
    ) -> Result<(Vec<Hit>, u32), String> {
        if threads > self.max_threads
            || filters as usize * 64 > self.buffers[FILTERS].size
            || words > 8
        {
            return Err("internal error: invalid GPU dispatch".into());
        }
        self.write(HITS, &[0])?;
        let (mut groups, mut threads, mut filters, mut words) = (groups, threads, filters, words);
        let [mut table, mut filter_words, mut centers, mut hits] =
            [TABLE, FILTERS, CENTERS, HITS].map(|b| self.buffers[b].ptr);
        let mut args: [*mut c_void; 8] = [
            (&raw mut groups).cast(),
            (&raw mut threads).cast(),
            (&raw mut filters).cast(),
            (&raw mut words).cast(),
            (&raw mut table).cast(),
            (&raw mut filter_words).cast(),
            (&raw mut centers).cast(),
            (&raw mut hits).cast(),
        ];
        // SAFETY: the arguments match the kernel's parameters (four u32,
        // then four device pointers to buffers of the sizes it expects).
        self.d.check("cuLaunchKernel", unsafe {
            (self.d.launch_kernel)(
                self.function,
                threads.div_ceil(WORKGROUP_SIZE),
                1,
                1,
                WORKGROUP_SIZE,
                1,
                1,
                0,
                std::ptr::null_mut(),
                args.as_mut_ptr(),
                std::ptr::null_mut(),
            )
        })?;

        // Wait for the kernel without blocking forever if the GPU hangs.
        let deadline = Instant::now() + POLL_TIMEOUT;
        loop {
            // SAFETY: the default stream of the current context.
            match unsafe { (self.d.stream_query)(std::ptr::null_mut()) } {
                driver::CUDA_SUCCESS => break,
                driver::CUDA_ERROR_NOT_READY if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_micros(500));
                }
                driver::CUDA_ERROR_NOT_READY => {
                    self.busy.set(true);
                    if let Ok(mut stuck) = STUCK.lock() {
                        stuck.push(self.ordinal);
                    }
                    return Err("the GPU did not respond".into());
                }
                e => self.d.check("the GPU program", e)?,
            }
        }
        Ok(super::parse_hits(&self.read(HITS, HITS_WORDS)?))
    }

    fn busy(&self) -> bool {
        self.busy.get()
    }

    #[cfg(test)]
    fn read_centers(&self, threads: u32) -> Result<Vec<u32>, String> {
        self.read(CENTERS, threads as usize * 16)
    }
}

impl Drop for CudaGpu {
    fn drop(&mut self) {
        // Freeing memory waits for the kernel; if it may still be running,
        // leave everything allocated rather than block forever.
        if self.busy.get() {
            return;
        }
        // Errors are ignored: there is nothing left to do with them.
        // SAFETY: everything below was created by `new` in this context and
        // is released once.
        unsafe {
            if (self.d.ctx_set_current)(self.context) == driver::CUDA_SUCCESS {
                for b in &self.buffers {
                    (self.d.mem_free)(b.ptr);
                }
                if !self.module.is_null() {
                    (self.d.module_unload)(self.module);
                }
            }
            (self.d.primary_ctx_release)(self.device);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn ptx_matches_kernel_constants() {
        let source = include_str!("kernel.cu");
        assert!(source.contains(&format!("#define GROUP {}\n", super::super::GROUP)));
        assert!(source.contains(&format!("#define MAX_HITS {}\n", super::super::MAX_HITS)));
        assert!(super::PTX.contains(".entry search("));
        assert!(super::PTX.contains(".target sm_50"));
    }
}
