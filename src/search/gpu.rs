//! GPU worker (placeholder until the wgpu backend lands).

use super::{GpuInfo, GpuMode, Shared};

pub(super) fn list() -> Vec<GpuInfo> {
    Vec::new()
}

pub(super) fn run(shared: &Shared, mode: GpuMode, cpu_threads: usize) {
    let message = "GPU support is not available in this build".to_string();
    if mode == GpuMode::On || cpu_threads == 0 {
        shared.fail(message);
    }
}
