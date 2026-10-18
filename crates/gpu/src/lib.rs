//! # Native GPU compute renderer
//!
//! [`GpuRenderer`] compiles typed [`Program`] fields into WGSL, dispatches compute
//! work, and reads back linear premultiplied float pixels. No surface or window is
//! required. Prefer the `synarith` facade for backend selection, automatic CPU
//! fallback, pipeline caching, and streamed PNG export.
//!
//! # Data flow
//!
//! Each scheduled expression becomes a WGSL function with explicit position and footprint
//! inputs. Shared graph nodes become reusable temporaries; unreachable functions are omitted.
//! Lazy sampling operations call source functions at transformed coordinates.
//! Uniforms contain view mapping, region, sampling quality, time, and seed; a
//! separate storage buffer contains typed parameter components. Workgroups of
//! 8×8 threads write float pixels, which are copied to a mapped staging buffer.
//!
//! [`GpuRenderer::compile`] returns a pipeline for one output; reuse it across
//! frames and parameter overrides. Keep the pipeline, source graph, and device
//! paired. Calls on a renderer are serialized to preserve device error scopes.
//! Buffer and dispatch limits are checked before allocation. GPU work is
//! synchronous; submitted dispatches finish before readback cancellation returns.
//!
//! # Backend parity
//!
//! All operations in [`synarith_core::operations`] are implemented. Integer hashes
//! match the CPU implementation, while floating-point and transcendental results
//! may differ. Tests compare representative cases with a 0.004 absolute tolerance
//! per linear premultiplied RGBA component and tile agreement within 0.00001.
//! Set `SYAR_REQUIRE_GPU=1` when running tests to require actual GPU execution;
//! without it, unavailable adapters produce an explicit skip message.
#![deny(missing_docs)]
mod shader;
use synarith_core::Program;
use synarith_cpu::{Mapping, Pixel, Region, RenderOptions};
use wgpu::util::DeviceExt;

/// Native compute device and synchronous render/readback executor.
///
/// Calls on one renderer are serialized so device error scopes cannot
/// interleave. No window or surface is needed.
pub struct GpuRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    execution: std::sync::Mutex<()>,
    /// Selected adapter name and graphics backend, suitable for status messages.
    pub adapter_name: String,
}
/// Compiled compute pipeline for one output of one source graph.
///
/// Keep it paired with the same [`Program`] and [`GpuRenderer`] used to compile
/// it. Parameter overrides, time, seed, and sampling options may change.
pub struct GpuProgram {
    pipeline: wgpu::ComputePipeline,
    output: String,
}
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Settings {
    size: [u32; 4],
    geom: [f32; 4],
    viewbox: [f32; 4],
    state: [u32; 4],
    clock: [f32; 4],
}
impl GpuRenderer {
    /// Select a native adapter and request a compute device with baseline limits.
    ///
    /// # Errors
    /// Returns an error when no compatible adapter is available or device creation fails.
    pub fn new() -> Result<Self, String> {
        pollster::block_on(async {
            let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    compatible_surface: None,
                    force_fallback_adapter: false,
                })
                .await
                .ok_or("no compatible GPU adapter")?;
            let info = adapter.get_info();
            let (device, queue) = adapter
                .request_device(
                    &wgpu::DeviceDescriptor {
                        label: Some("Synarith"),
                        required_features: wgpu::Features::empty(),
                        required_limits: wgpu::Limits::default(),
                        memory_hints: wgpu::MemoryHints::MemoryUsage,
                    },
                    None,
                )
                .await
                .map_err(|e| format!("GPU device creation: {e}"))?;
            Ok(Self {
                device,
                queue,
                execution: std::sync::Mutex::new(()),
                adapter_name: format!("{} ({:?})", info.name, info.backend),
            })
        })
    }
    fn scoped<T>(&self, action: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
        // Error scopes belong to the device, so keep concurrent callers from interleaving them.
        let _guard = self
            .execution
            .lock()
            .map_err(|_| "GPU execution lock poisoned")?;
        for filter in [
            wgpu::ErrorFilter::OutOfMemory,
            wgpu::ErrorFilter::Internal,
            wgpu::ErrorFilter::Validation,
        ] {
            self.device.push_error_scope(filter);
        }
        let result = action();
        self.device.poll(wgpu::Maintain::Wait);
        let mut errors = Vec::new();
        for _ in 0..3 {
            if let Some(error) = pollster::block_on(self.device.pop_error_scope()) {
                errors.push(error.to_string());
            }
        }
        if !errors.is_empty() {
            return Err(format!("GPU: {}", errors.join("; ")));
        }
        result
    }
    /// Compile one named output of a validated graph into WGSL and a compute pipeline.
    ///
    /// The source graph must come from [`synarith_core::compile`]. Retain the
    /// returned pipeline for subsequent frames and keep it paired with this device.
    ///
    /// # Errors
    /// Returns unknown-output, shader validation, pipeline, or device-resource errors.
    pub fn compile(&self, program: &Program, output: &str) -> Result<GpuProgram, String> {
        let source = shader::generate(program, output)?;
        self.scoped(|| {
            let module = self
                .device
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("Synarith fields"),
                    source: wgpu::ShaderSource::Wgsl(source.into()),
                });
            // Keep all bindings even when a document uses no parameters or uniforms.
            let entries = [
                wgpu::BufferBindingType::Uniform,
                wgpu::BufferBindingType::Storage { read_only: true },
                wgpu::BufferBindingType::Storage { read_only: false },
            ]
            .into_iter()
            .enumerate()
            .map(|(binding, ty)| wgpu::BindGroupLayoutEntry {
                binding: binding as u32,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            })
            .collect::<Vec<_>>();
            let bindings = self
                .device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("Synarith bindings"),
                    entries: &entries,
                });
            let layout = self
                .device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: None,
                    bind_group_layouts: &[&bindings],
                    push_constant_ranges: &[],
                });
            let pipeline = self
                .device
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some(output),
                    layout: Some(&layout),
                    module: &module,
                    entry_point: Some("main"),
                    compilation_options: Default::default(),
                    cache: None,
                });
            Ok(GpuProgram {
                pipeline,
                output: output.into(),
            })
        })
    }
    /// Dispatch a region and read back row-major linear premultiplied RGBA pixels.
    ///
    /// `compiled` must correspond to `program`, the selected output, and this device.
    /// Coordinates use the full image mapping. Cancellation is checked before
    /// dispatch and after readback; submitted GPU work is allowed to finish.
    ///
    /// # Errors
    /// Returns invalid settings, mismatched output, device buffer/dispatch limits,
    /// allocation, validation, mapping/readback, or cancellation errors.
    pub fn render_region(
        &self,
        program: &Program,
        compiled: &GpuProgram,
        options: &RenderOptions,
        region: Region,
    ) -> Result<Vec<Pixel>, String> {
        options.validate(program, region)?;
        if compiled.output != options.output {
            return Err("GPU program output does not match render options".into());
        }
        if options.cancellation.is_cancelled() {
            return Err("render cancelled".into());
        }
        let bytes = u64::from(region.width) * u64::from(region.height) * 16;
        let limits = self.device.limits();
        let groups = [region.width.div_ceil(8), region.height.div_ceil(8)];
        if bytes > u64::from(limits.max_storage_buffer_binding_size)
            || bytes > limits.max_buffer_size
            || groups
                .iter()
                .any(|&n| n > limits.max_compute_workgroups_per_dimension)
        {
            return Err("region exceeds GPU limits; render smaller tiles".into());
        }
        let mapping = Mapping::new(
            options.viewport.unwrap_or(program.viewbox),
            options.width,
            options.height,
        );
        let settings = Settings {
            size: [options.width, options.height, region.x, region.y],
            geom: [
                mapping.origin[0],
                mapping.origin[1],
                mapping.units_per_pixel,
                0.,
            ],
            viewbox: options.viewport.unwrap_or(program.viewbox),
            state: [
                region.width,
                region.height,
                options.samples,
                options.seed.unwrap_or(program.seed),
            ],
            clock: [options.time, 0., 0., 0.],
        };
        let values = if options.parameters.is_empty() {
            program.defaults()
        } else {
            options.parameters.clone()
        };
        let mut parameters = values.iter().map(|v| v.data).collect::<Vec<_>>();
        if parameters.is_empty() {
            parameters.push([0.; 4]);
        }
        if parameters.len() as u64 * 16 > u64::from(limits.max_storage_buffer_binding_size) {
            return Err("parameters exceed GPU buffer limits".into());
        }
        self.scoped(|| {
            let uniform = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: None,
                    contents: bytemuck::bytes_of(&settings),
                    usage: wgpu::BufferUsages::UNIFORM,
                });
            let params = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: None,
                    contents: bytemuck::cast_slice(&parameters),
                    usage: wgpu::BufferUsages::STORAGE,
                });
            let output = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: bytes,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });
            let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: bytes,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let entries = [&uniform, &params, &output]
                .into_iter()
                .enumerate()
                .map(|(binding, buffer)| wgpu::BindGroupEntry {
                    binding: binding as u32,
                    resource: buffer.as_entire_binding(),
                })
                .collect::<Vec<_>>();
            let bindings = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &compiled.pipeline.get_bind_group_layout(0),
                entries: &entries,
            });
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: None,
                    timestamp_writes: None,
                });
                pass.set_pipeline(&compiled.pipeline);
                pass.set_bind_group(0, &bindings, &[]);
                pass.dispatch_workgroups(groups[0], groups[1], 1);
            }
            encoder.copy_buffer_to_buffer(&output, 0, &staging, 0, bytes);
            self.queue.submit(Some(encoder.finish()));
            let (tx, rx) = std::sync::mpsc::channel();
            staging
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |result| {
                    let _ = tx.send(result);
                });
            self.device.poll(wgpu::Maintain::Wait);
            rx.recv()
                .map_err(|e| e.to_string())?
                .map_err(|e| format!("GPU readback: {e}"))?;
            if options.cancellation.is_cancelled() {
                staging.unmap();
                return Err("render cancelled".into());
            }
            let data = staging.slice(..).get_mapped_range();
            let mut pixels = Vec::new();
            pixels
                .try_reserve_exact((bytes / 16) as usize)
                .map_err(|_| "unable to allocate GPU readback")?;
            pixels.extend_from_slice(bytemuck::cast_slice::<u8, Pixel>(&data));
            drop(data);
            staging.unmap();
            Ok(pixels)
        })
    }
}
