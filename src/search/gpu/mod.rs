//! GPU worker: the same symmetric key walk as the CPU (see
//! [`crate::curve::walk`]), written in WGSL and run through wgpu on any
//! Vulkan, DirectX 12 or Metal device — NVIDIA, AMD, Intel or Apple.
//!
//! The GPU only reports *candidates* (keys whose y coordinate passes a prefix
//! filter). Every candidate is re-derived and verified on the CPU by
//! [`Shared::submit`], so a GPU or driver problem can at worst miss matches;
//! it can never produce a wrong key.

mod shader;

use std::collections::HashSet;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use wgpu::util::DeviceExt;

use crate::curve::field::{Fe, batch_invert};
use crate::curve::point::ExtendedPoint;
use crate::curve::walk::{WalkTable, group_offset};
use crate::pattern::PrefixFilter;

use super::{Event, GpuInfo, GpuMode, Shared, Walk, software_gpu_allowed};

/// Half-size of a GPU key group (129 keys per group).
const GROUP: usize = 64;
const KEYS_PER_GROUP: u64 = 2 * GROUP as u64 + 1;
/// Must match `MAX_HITS` in kernel.wgsl.
const MAX_HITS: usize = 1024;
const WORKGROUP_SIZE: u32 = 64;
const MAX_GROUPS_PER_DISPATCH: u32 = 64;
/// Groups each invocation walks before the walk is re-seeded.
const GROUPS_PER_WALK: u64 = 1 << 16;
/// Aim for short dispatches: the desktop stays responsive and we stay far
/// below the operating system's GPU watchdog (2 seconds on Windows).
const TARGET_DISPATCH: Duration = Duration::from_millis(40);
const POLL_TIMEOUT: Duration = Duration::from_secs(20);

fn type_rank(t: wgpu::DeviceType) -> u8 {
    match t {
        wgpu::DeviceType::DiscreteGpu => 0,
        wgpu::DeviceType::IntegratedGpu => 1,
        wgpu::DeviceType::VirtualGpu => 2,
        wgpu::DeviceType::Other => 3,
        wgpu::DeviceType::Cpu => 4,
    }
}

fn backend_name(backend: wgpu::Backend) -> &'static str {
    match backend {
        wgpu::Backend::Vulkan => "Vulkan",
        wgpu::Backend::Dx12 => "DirectX 12",
        wgpu::Backend::Metal => "Metal",
        _ => "other",
    }
}

fn backend_rank(backend: wgpu::Backend) -> u8 {
    match backend {
        wgpu::Backend::Vulkan => 0,
        wgpu::Backend::Metal => 1,
        wgpu::Backend::Dx12 => 2,
        _ => 3,
    }
}

/// Compute-capable adapters, best first, one per physical device.
fn adapters() -> Vec<wgpu::Adapter> {
    let backends = wgpu::Backends::VULKAN | wgpu::Backends::DX12 | wgpu::Backends::METAL;
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let mut adapters = pollster::block_on(instance.enumerate_adapters(backends));
    adapters.retain(|a| {
        a.get_downlevel_capabilities()
            .flags
            .contains(wgpu::DownlevelFlags::COMPUTE_SHADERS)
    });
    adapters.sort_by_key(|a| {
        let info = a.get_info();
        (type_rank(info.device_type), backend_rank(info.backend))
    });
    let mut seen = HashSet::new();
    adapters.retain(|a| seen.insert(a.get_info().name));
    adapters
}

pub(super) fn list() -> Vec<GpuInfo> {
    adapters()
        .iter()
        .map(|a| {
            let info = a.get_info();
            GpuInfo {
                name: info.name,
                backend: backend_name(info.backend).to_string(),
                discrete: info.device_type == wgpu::DeviceType::DiscreteGpu,
                software: info.device_type == wgpu::DeviceType::Cpu,
            }
        })
        .collect()
}

/// The adapter to use: `Auto` only takes dedicated GPUs, `On` anything that
/// is not a software renderer (unless explicitly allowed).
fn choose(mode: GpuMode, software: bool) -> Option<wgpu::Adapter> {
    adapters()
        .into_iter()
        .find(|a| match a.get_info().device_type {
            wgpu::DeviceType::DiscreteGpu => true,
            wgpu::DeviceType::Cpu => mode == GpuMode::On && software,
            _ => mode == GpuMode::On,
        })
}

/// Initial (and maximum) number of GPU invocations per dispatch.
fn max_threads(device_type: wgpu::DeviceType) -> u32 {
    match device_type {
        wgpu::DeviceType::DiscreteGpu => 32_768,
        wgpu::DeviceType::IntegratedGpu => 8_192,
        wgpu::DeviceType::VirtualGpu | wgpu::DeviceType::Other => 4_096,
        wgpu::DeviceType::Cpu => 256,
    }
}

pub(super) fn run(shared: &Shared, mode: GpuMode, cpu_threads: usize) {
    let unavailable = |reason: &str| {
        if mode == GpuMode::On || cpu_threads == 0 {
            shared.fail(format!("the GPU cannot be used: {reason}"));
        } else {
            shared.send(Event::Info(format!(
                "Not using the GPU: {reason}. Searching on the CPU only."
            )));
        }
    };

    if shared.matcher.anywhere() {
        return unavailable("it only supports patterns at the start of the address");
    }
    let Some(adapter) = choose(mode, software_gpu_allowed()) else {
        return unavailable(if mode == GpuMode::Auto {
            "no dedicated graphics card was found"
        } else {
            "no compatible graphics card was found"
        });
    };
    let info = adapter.get_info();
    let gpu = match Gpu::new(
        &adapter,
        max_threads(info.device_type),
        shared.filters.len(),
    ) {
        Ok(gpu) => gpu,
        Err(e) => return unavailable(&e),
    };
    shared.send(Event::Info(format!(
        "Using the GPU: {} ({})",
        info.name,
        backend_name(info.backend)
    )));
    if let Err(e) = gpu.search(shared) {
        if cpu_threads == 0 {
            shared.fail(format!("the GPU stopped working: {e}"));
        } else {
            shared.send(Event::Warning(format!(
                "the GPU stopped working ({e}); continuing on the CPU"
            )));
        }
    }
}

/// One candidate reported by the kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Hit {
    thread: u32,
    group: u32,
    index: u32,
}

/// Device, pipeline and buffers.
struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::ComputePipeline,
    bind_group: wgpu::BindGroup,
    params: wgpu::Buffer,
    filters: wgpu::Buffer,
    centers: wgpu::Buffer,
    hits: wgpu::Buffer,
    readback: wgpu::Buffer,
    max_threads: u32,
    error: Arc<Mutex<Option<String>>>,
}

fn to_bytes(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

impl Gpu {
    fn new(adapter: &wgpu::Adapter, max_threads: u32, max_filters: usize) -> Result<Gpu, String> {
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("torv3-vanity"),
            required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
            ..Default::default()
        }))
        .map_err(|e| format!("could not open the device: {e}"))?;

        let error = Arc::new(Mutex::new(None::<String>));
        {
            let error = Arc::clone(&error);
            device.on_uncaptured_error(Arc::new(move |e: wgpu::Error| {
                if let Ok(mut slot) = error.lock() {
                    slot.get_or_insert_with(|| e.to_string());
                }
            }));
        }
        {
            let error = Arc::clone(&error);
            device.set_device_lost_callback(move |_, message| {
                if let Ok(mut slot) = error.lock() {
                    slot.get_or_insert_with(|| format!("device lost: {message}"));
                }
            });
        }

        let internal = device.push_error_scope(wgpu::ErrorFilter::Internal);
        let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("torv3-vanity kernel"),
            source: wgpu::ShaderSource::Wgsl(shader::source(GROUP).into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("torv3-vanity search"),
            layout: None,
            module: &module,
            entry_point: Some("search"),
            compilation_options: Default::default(),
            cache: None,
        });
        if let Some(e) = pollster::block_on(validation.pop()) {
            return Err(format!("the GPU program failed to compile: {e}"));
        }
        if let Some(e) = pollster::block_on(internal.pop()) {
            return Err(format!(
                "the graphics driver could not compile the GPU program: {e}"
            ));
        }

        let storage = |label, size: usize, extra| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: size as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | extra,
                mapped_at_creation: false,
            })
        };
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("params"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let table = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("table"),
            contents: &to_bytes(&table_words()),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let filters = storage(
            "filters",
            max_filters.max(1) * 64,
            wgpu::BufferUsages::empty(),
        );
        let centers = storage(
            "centers",
            max_threads as usize * 64,
            wgpu::BufferUsages::COPY_SRC,
        );
        let hits_size = (1 + 3 * MAX_HITS) * 4;
        let hits = storage("hits", hits_size, wgpu::BufferUsages::COPY_SRC);
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: hits_size as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("torv3-vanity bindings"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: params.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: table.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: filters.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: centers.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: hits.as_entire_binding(),
                },
            ],
        });

        Ok(Gpu {
            device,
            queue,
            pipeline,
            bind_group,
            params,
            filters,
            centers,
            hits,
            readback,
            max_threads,
            error,
        })
    }

    fn check_error(&self) -> Result<(), String> {
        match self.error.lock().ok().and_then(|mut e| e.take()) {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    fn upload_centers(&self, words: &[u32]) {
        self.queue.write_buffer(&self.centers, 0, &to_bytes(words));
    }

    fn upload_filters(&self, filters: &[[u32; 16]]) {
        let words: Vec<u32> = filters.iter().flatten().copied().collect();
        if !words.is_empty() {
            self.queue.write_buffer(&self.filters, 0, &to_bytes(&words));
        }
    }

    /// Run `groups` groups on `threads` invocations and return the hits.
    fn dispatch(
        &self,
        groups: u32,
        threads: u32,
        filters: u32,
        words: u32,
    ) -> Result<Vec<Hit>, String> {
        self.queue.write_buffer(
            &self.params,
            0,
            &to_bytes(&[groups, threads, filters, words]),
        );
        self.queue.write_buffer(&self.hits, 0, &[0u8; 4]);
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.dispatch_workgroups(threads.div_ceil(WORKGROUP_SIZE), 1, 1);
        }
        encoder.copy_buffer_to_buffer(&self.hits, 0, &self.readback, 0, self.readback.size());
        let submission = self.queue.submit([encoder.finish()]);

        let slice = self.readback.slice(..);
        let (tx, rx) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(POLL_TIMEOUT),
            })
            .map_err(|e| format!("the GPU did not respond: {e}"))?;
        self.check_error()?;
        rx.recv()
            .map_err(|_| "the GPU result was lost".to_string())?
            .map_err(|e| format!("could not read the GPU result: {e}"))?;

        let hits = {
            let data = slice
                .get_mapped_range()
                .map_err(|e| format!("could not read the GPU result: {e}"))?;
            let word =
                |i: usize| u32::from_le_bytes(data[i * 4..i * 4 + 4].try_into().expect("4 bytes"));
            let count = (word(0) as usize).min(MAX_HITS);
            (0..count)
                .map(|i| Hit {
                    thread: word(1 + 3 * i),
                    group: word(2 + 3 * i),
                    index: word(3 + 3 * i),
                })
                .collect()
        };
        self.readback.unmap();
        Ok(hits)
    }

    fn search(&self, shared: &Shared) -> Result<(), String> {
        let mut threads = self.max_threads;
        let mut groups = 1u32;
        let mut filters = ActiveFilters::default();
        let mut walk: Option<GpuWalk> = None;

        while !shared.should_stop() {
            if filters.refresh(shared) {
                self.upload_filters(&filters.words);
            }
            if filters.words.is_empty() {
                return Ok(());
            }
            if walk
                .as_ref()
                .is_none_or(|w| w.groups_done + u64::from(groups) > GROUPS_PER_WALK)
            {
                let new = GpuWalk::new()?;
                self.upload_centers(&new.centers(threads));
                walk = Some(new);
            }
            let current = walk.as_mut().expect("walk was just created");

            let started = Instant::now();
            let hits = self.dispatch(
                groups,
                threads,
                filters.words.len() as u32,
                filters.max_words,
            )?;
            let elapsed = started.elapsed();
            shared.add_gpu_keys(u64::from(threads) * u64::from(groups) * KEYS_PER_GROUP);

            let mut saved = false;
            for hit in hits {
                let key = current.key(hit, groups)?;
                if shared.submit(&current.walk.scalar(key), current.walk.start.hash_prefix()) {
                    // Keys saved from one walk would be related; start over.
                    saved = true;
                    break;
                }
            }
            current.groups_done += u64::from(groups);
            if saved {
                walk = None;
            }

            // Keep each dispatch close to the target duration.
            if elapsed < TARGET_DISPATCH / 2 && groups < MAX_GROUPS_PER_DISPATCH {
                groups *= 2;
            } else if elapsed > TARGET_DISPATCH * 2 {
                if groups > 1 {
                    groups /= 2;
                } else if threads > WORKGROUP_SIZE {
                    threads = (threads / 2).max(WORKGROUP_SIZE);
                    walk = None;
                }
            }
        }
        Ok(())
    }
}

/// The GPU copy of the start-anchored filters still wanted.
#[derive(Default)]
struct ActiveFilters {
    generation: Option<u64>,
    words: Vec<[u32; 16]>,
    max_words: u32,
}

impl ActiveFilters {
    /// Returns true if the filters changed.
    fn refresh(&mut self, shared: &Shared) -> bool {
        let generation = shared.generation.load(std::sync::atomic::Ordering::Acquire);
        if self.generation == Some(generation) {
            return false;
        }
        self.generation = Some(generation);
        self.words = shared
            .filters
            .iter()
            .enumerate()
            .filter(|&(i, f)| shared.wants(i) && f.words > 0)
            .map(|(_, f)| filter_words(f))
            .collect();
        self.max_words = self
            .words
            .iter()
            .map(|w| {
                w[..8]
                    .iter()
                    .rposition(|&m| m != 0)
                    .map_or(0, |i| i as u32 + 1)
            })
            .max()
            .unwrap_or(0);
        true
    }
}

/// Convert a [`PrefixFilter`] (big-endian words over the key bytes) into the
/// kernel's little-endian words over the packed y coordinate.
fn filter_words(filter: &PrefixFilter) -> [u32; 16] {
    let (mut mask, mut value) = ([0u8; 32], [0u8; 32]);
    for w in 0..4 {
        mask[w * 8..w * 8 + 8].copy_from_slice(&filter.mask[w].to_be_bytes());
        value[w * 8..w * 8 + 8].copy_from_slice(&filter.value[w].to_be_bytes());
    }
    let mut out = [0u32; 16];
    out[..8].copy_from_slice(&shader::words(&mask));
    out[8..].copy_from_slice(&shader::words(&value));
    out
}

/// `(x, y, d·x·y)` of `i·8B` for `i = 1..=GROUP`, then the step to the next
/// center, as 13-bit limbs.
fn table_words() -> Vec<u32> {
    let table = WalkTable::new(GROUP);
    let mut out = Vec::with_capacity((GROUP + 1) * 60);
    for entry in table.entries.iter().chain(std::iter::once(&table.advance)) {
        for f in [entry.x, entry.y, entry.e] {
            out.extend(shader::limbs(&f.to_bytes()));
        }
    }
    out
}

/// A random walk split across the GPU invocations: invocation `t` walks the
/// keys `t·K ..` of the walk, where `K` is the number of keys per invocation.
struct GpuWalk {
    walk: Walk,
    /// Groups each invocation has completed.
    groups_done: u64,
}

impl GpuWalk {
    const KEYS_PER_THREAD: u64 = GROUPS_PER_WALK * KEYS_PER_GROUP;

    fn new() -> Result<GpuWalk, String> {
        Ok(GpuWalk {
            walk: Walk::random()?,
            groups_done: 0,
        })
    }

    /// Walk index of the first center of invocation `thread`.
    fn first_center(thread: u32) -> u64 {
        u64::from(thread) * Self::KEYS_PER_THREAD + GROUP as u64
    }

    /// Walk index of the key reported by `hit` in a dispatch of `groups`.
    fn key(&self, hit: Hit, groups: u32) -> Result<u64, String> {
        if hit.group >= groups || hit.index as u64 >= KEYS_PER_GROUP {
            return Err("the GPU returned an invalid result".into());
        }
        let center = Self::first_center(hit.thread)
            + (self.groups_done + u64::from(hit.group)) * KEYS_PER_GROUP;
        center
            .checked_add_signed(group_offset(hit.index as usize))
            .ok_or_else(|| "the GPU returned an invalid result".into())
    }

    /// Packed affine starting centers for `threads` invocations.
    fn centers(&self, threads: u32) -> Vec<u32> {
        let start = ExtendedPoint::mul_base(&self.walk.scalar(GROUP as u64));
        let mut stride = [0u8; 32];
        stride[..8].copy_from_slice(&(Self::KEYS_PER_THREAD * 8).to_le_bytes());
        let step = ExtendedPoint::mul_base(&stride).to_affine_niels();

        let mut points = Vec::with_capacity(threads as usize);
        let mut p = start;
        for _ in 0..threads {
            points.push(p);
            p = p.add_niels(&step);
        }
        let mut zs: Vec<Fe> = points.iter().map(|p| p.z).collect();
        let mut scratch = vec![Fe::ZERO; zs.len()];
        batch_invert(&mut zs, &mut scratch);

        let mut out = Vec::with_capacity(threads as usize * 16);
        for (p, zinv) in points.iter().zip(&zs) {
            out.extend(shader::words(&p.x.mul(*zinv).to_bytes()));
            out.extend(shader::words(&p.y.mul(*zinv).to_bytes()));
        }
        out
    }
}

#[cfg(test)]
mod tests;
