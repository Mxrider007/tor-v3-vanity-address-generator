//! The few CUDA driver API functions the search needs, loaded at run time
//! from the NVIDIA driver (`nvcuda.dll` on Windows, `libcuda.so.1` on
//! Linux). Neither building nor running the program needs the CUDA toolkit,
//! and without an NVIDIA driver CUDA is simply reported as unavailable.

use std::ffi::{CStr, c_char, c_int, c_uint, c_void};
use std::sync::OnceLock;

pub(super) type CuResult = c_int;
pub(super) type CuDevice = c_int;
pub(super) type CuContext = *mut c_void;
pub(super) type CuModule = *mut c_void;
pub(super) type CuFunction = *mut c_void;
pub(super) type CuStream = *mut c_void;
pub(super) type CuDevicePtr = u64;

pub(super) const CUDA_SUCCESS: CuResult = 0;
const CUDA_ERROR_INSUFFICIENT_DRIVER: CuResult = 35;
const CUDA_ERROR_NO_DEVICE: CuResult = 100;
pub(super) const CUDA_ERROR_NOT_READY: CuResult = 600;

pub(super) const ATTRIBUTE_MULTIPROCESSOR_COUNT: c_int = 16;
pub(super) const ATTRIBUTE_INTEGRATED: c_int = 18;
pub(super) const ATTRIBUTE_COMPUTE_CAPABILITY_MAJOR: c_int = 75;
pub(super) const ATTRIBUTE_COMPUTE_CAPABILITY_MINOR: c_int = 76;

pub(super) const JIT_ERROR_LOG_BUFFER: c_int = 5;
pub(super) const JIT_ERROR_LOG_BUFFER_SIZE_BYTES: c_int = 6;

macro_rules! driver_api {
    ($($field:ident: [$($symbol:literal),+] fn($($arg:ty),*);)*) => {
        /// Entry points of the CUDA driver.
        #[allow(non_snake_case)]
        pub(super) struct Driver {
            $(pub(super) $field: unsafe extern "system" fn($($arg),*) -> CuResult,)*
            /// Keeps the functions above loaded.
            _library: libloading::Library,
        }

        impl Driver {
            /// # Safety
            /// `library` must be the CUDA driver library, whose functions
            /// have the signatures declared above.
            unsafe fn from_library(library: libloading::Library) -> Result<Driver, String> {
                // SAFETY: per this function's contract, each symbol has the
                // declared signature; the library outlives the pointers.
                unsafe {
                    Ok(Driver {
                        // The first of the listed names that the driver has.
                        $($field: [$($symbol),+]
                            .iter()
                            .find_map(|name| {
                                library
                                    .get::<unsafe extern "system" fn($($arg),*) -> CuResult>(
                                        format!("{name}\0").as_bytes(),
                                    )
                                    .ok()
                                    .map(|f| *f)
                            })
                            .ok_or_else(|| {
                                format!("the NVIDIA driver is too old (it has no {})", [$($symbol),+][0])
                            })?,)*
                        _library: library,
                    })
                }
            }
        }
    };
}

driver_api! {
    init: ["cuInit"] fn(c_uint);
    device_get_count: ["cuDeviceGetCount"] fn(*mut c_int);
    device_get: ["cuDeviceGet"] fn(*mut CuDevice, c_int);
    device_get_name: ["cuDeviceGetName"] fn(*mut c_char, c_int, CuDevice);
    device_get_attribute: ["cuDeviceGetAttribute"] fn(*mut c_int, c_int, CuDevice);
    primary_ctx_retain: ["cuDevicePrimaryCtxRetain"] fn(*mut CuContext, CuDevice);
    // Older drivers (before CUDA 11) only have the first version.
    primary_ctx_release: ["cuDevicePrimaryCtxRelease_v2", "cuDevicePrimaryCtxRelease"] fn(CuDevice);
    ctx_set_current: ["cuCtxSetCurrent"] fn(CuContext);
    module_load_data_ex: ["cuModuleLoadDataEx"] fn(*mut CuModule, *const c_void, c_uint, *mut c_int, *mut *mut c_void);
    module_get_function: ["cuModuleGetFunction"] fn(*mut CuFunction, CuModule, *const c_char);
    module_unload: ["cuModuleUnload"] fn(CuModule);
    mem_alloc: ["cuMemAlloc_v2"] fn(*mut CuDevicePtr, usize);
    mem_free: ["cuMemFree_v2"] fn(CuDevicePtr);
    memcpy_htod: ["cuMemcpyHtoD_v2"] fn(CuDevicePtr, *const c_void, usize);
    memcpy_dtoh: ["cuMemcpyDtoH_v2"] fn(*mut c_void, CuDevicePtr, usize);
    launch_kernel: ["cuLaunchKernel"] fn(CuFunction, c_uint, c_uint, c_uint, c_uint, c_uint, c_uint, c_uint, CuStream, *mut *mut c_void, *mut *mut c_void);
    stream_query: ["cuStreamQuery"] fn(CuStream);
    get_error_name: ["cuGetErrorName"] fn(CuResult, *mut *const c_char);
}

/// The initialized driver, loaded on first use.
pub(super) fn driver() -> Result<&'static Driver, String> {
    static DRIVER: OnceLock<Result<Driver, String>> = OnceLock::new();
    DRIVER.get_or_init(load).as_ref().map_err(Clone::clone)
}

fn load() -> Result<Driver, String> {
    let library = open_library()?;
    // SAFETY: `open_library` only opens the NVIDIA driver library.
    let driver = unsafe { Driver::from_library(library)? };
    // SAFETY: cuInit has no preconditions.
    match unsafe { (driver.init)(0) } {
        CUDA_ERROR_NO_DEVICE => Err("no NVIDIA graphics card was found".to_string()),
        CUDA_ERROR_INSUFFICIENT_DRIVER => Err("the NVIDIA driver is too old".to_string()),
        result => driver.check("cuInit", result).map(|()| driver),
    }
}

#[cfg(windows)]
fn open_library() -> Result<libloading::Library, String> {
    use libloading::os::windows::{LOAD_LIBRARY_SEARCH_SYSTEM32, Library};
    // Only from System32, where the driver installs it, so that a DLL with
    // the same name next to the program or in the current folder is never
    // loaded instead.
    // SAFETY: loading the NVIDIA driver runs no code that could break Rust's
    // invariants.
    unsafe { Library::load_with_flags("nvcuda.dll", LOAD_LIBRARY_SEARCH_SYSTEM32) }
        .map(Into::into)
        .map_err(|_| "no NVIDIA driver with CUDA support is installed".to_string())
}

#[cfg(target_os = "linux")]
fn open_library() -> Result<libloading::Library, String> {
    // SAFETY: as above.
    unsafe { libloading::Library::new("libcuda.so.1") }
        .map_err(|_| "no NVIDIA driver with CUDA support is installed".to_string())
}

#[cfg(not(any(windows, target_os = "linux")))]
fn open_library() -> Result<libloading::Library, String> {
    Err("CUDA is only available on Windows and Linux".to_string())
}

impl Driver {
    /// Turn a result code into an error naming the failed call.
    pub(super) fn check(&self, call: &str, result: CuResult) -> Result<(), String> {
        if result == CUDA_SUCCESS {
            return Ok(());
        }
        let mut name: *const c_char = std::ptr::null();
        // SAFETY: cuGetErrorName stores a pointer to a static string (or
        // leaves it null for unknown codes).
        let known = unsafe { (self.get_error_name)(result, &mut name) } == CUDA_SUCCESS;
        if known && !name.is_null() {
            // SAFETY: a NUL-terminated static string from the driver.
            let name = unsafe { CStr::from_ptr(name) }.to_string_lossy();
            Err(format!("{call} failed ({name})"))
        } else {
            Err(format!("{call} failed (error {result})"))
        }
    }
}
