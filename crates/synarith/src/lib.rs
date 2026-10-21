//! # Synarith — images synthesized through arithmetic
//!
//! Render continuous mathematical images from self-contained `.syar` text documents.
//! Start with [`load`], construct a [`Renderer`], then render float pixels or export PNG.
//! The [format guide](mod@format) covers syntax, operations, color, animation, and limits;
//! the [CLI manual](cli) covers the `syar` executable.
//!
//! # Quick start
//!
//! ```
//! use synarith::{load, Renderer, RenderOptions};
//!
//! let program = load(r##"
//! syar 1;
//! use std;
//! param scale: float = 6.0;
//! let clouds = fbm(p * scale + vec2(t * 0.2, 0.0), octaves: 6);
//! output main = mix(srgb("#13254b"), srgb("#f4c879"), clouds);
//! "##)?;
//! let mut renderer = Renderer::new(program);
//! let options = RenderOptions { width: 32, height: 24, time: 1.5, ..Default::default() };
//! let pixels = renderer.render(&options)?;
//! assert_eq!(pixels.len(), 32 * 24);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # Backends and features
//!
//! [`Renderer::new`] uses CPU evaluation without initializing graphics.
//! [`Renderer::with_backend`] selects [`Backend::Auto`], [`Backend::Cpu`], or
//! [`Backend::Gpu`]. Auto falls back to CPU on GPU initialization, compilation,
//! or rendering errors; display [`Renderer::take_warnings`] to explain why.
//! Required GPU mode returns those errors instead. Pipelines are cached per output;
//! parameters, time, seed, and dimensions can change without recompilation.
//!
//! The default `gpu` feature enables native acceleration through wgpu. Set
//! `default-features = false` on this dependency for a CPU-only library. The API
//! still accepts Auto with a visible fallback reason and rejects required GPU.
//!
//! Applications that already own a GPU device can enable only the `wgsl` feature:
//! it adds [`wgsl`], [`uniform_bytes`], [`parameter_data`], and [`WORKGROUP_SIZE`]
//! without wgpu, so they can generate the compute shaders and dispatch them on
//! their own device. The binding layout is described in `synarith_gpu`.
//!
//! # Rendering, color, and cancellation
//!
//! [`Pixel`] is linear premultiplied RGBA. [`rgba8`] converts it to straight sRGB
//! bytes. [`Renderer::render_region`] uses full-image coordinates, so separately
//! rendered tiles align. [`Renderer::export_png`] streams bounded strips to a
//! temporary file and persists it only after success.
//!
//! [`RenderOptions`] controls output name, dimensions, time in seconds, seed,
//! supersampling, parameters, and cancellation. Empty parameter values select
//! document defaults; [`apply_overrides`] creates a complete typed parameter set.
//! Clone [`Cancellation`] to cancel from another thread. CPU work checks between
//! pixel batches; submitted GPU work finishes before readback cancellation returns.
//!
//! # Architecture
//!
//! Three layers separate reusable algorithms from execution:
//!
//! 1. **User documents:** mathematical fields, functions, bounded loops, and outputs.
//! 2. **Standard library:** dependency-free `synarith-stdlib` data, entirely `.syar` source.
//! 3. **Compiler/runtime:** typed graph and structured functions → [`ExecutionPlan`] →
//!    CPU expression evaluation or WGSL/native GPU execution.
//!
//! Source imports use a cached dependency index, adding only required definitions.
//! Execution schedules evaluate shared expressions once in a sampling context;
//! they never reuse values across mutation, coordinates, or footprints. Loops stay
//! structured. CPU execution chooses registers for substantial sharing and direct
//! trees for small cases. It remains an interpreter with no machine-code JIT.
//! GPU emission uses explicit temporaries and omits unreachable functions.
//! [`Renderer`] caches CPU schedules and GPU pipelines by output across frames and
//! tiled exports. The low-level [`CpuProgram`] also supports prepared point sampling.
//!
//! | Crate | Responsibility |
//! | --- | --- |
//! | `synarith` | Public API, backend selection, overrides, tiled PNG export |
//! | `synarith-stdlib` | Portable noise, shapes, gradients, and effects written in Synarith |
//! | `synarith-core` | Source preservation, diagnostics, operation registry, compiler |
//! | `synarith-cpu` | Reference evaluator, point samples, regions, pixel conversion |
//! | `synarith-gpu` | WGSL compiler, native compute pipelines, readback |
//! | `synarith-cli` | `syar` command-line executable |
//! | `synarith-viewer` | Egui `syar-view`; live playback, source watching, and camera navigation |
//!
//! Only compiler-produced [`Program`] values satisfy renderer invariants. Manually
//! constructing or mutating a graph into an invalid state is unsupported. Numeric
//! results can differ across backends; the parity tests use representative cases
//! with a 0.004 absolute tolerance per linear RGBA component, not a universal bound.
#![deny(missing_docs)]

pub mod cli;
mod export;
pub use export::ImageFormat;
use std::{io::Write, path::Path};
#[doc(inline)]
pub use synarith_core::*;
#[doc(inline)]
pub use synarith_cpu::*;
#[cfg(feature = "wgsl")]
pub use synarith_gpu::{WORKGROUP_SIZE, parameter_data, uniform_bytes, wgsl};

/// Backend selection policy; the default policy is [`Auto`](Self::Auto).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Backend {
    /// Try GPU, falling back to CPU with a retrievable explanation.
    #[default]
    Auto,
    /// Use the parallel reference renderer without initializing graphics.
    Cpu,
    /// Require GPU execution; initialization or rendering failure is an error.
    Gpu,
}
impl std::str::FromStr for Backend {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, String> {
        match value {
            "auto" => Ok(Self::Auto),
            "cpu" => Ok(Self::Cpu),
            "gpu" => Ok(Self::Gpu),
            _ => Err("backend must be auto, cpu, or gpu".into()),
        }
    }
}
/// Owns an immutable compiled graph and reusable backend resources.
///
/// Use [`Self::new`] for CPU rendering or [`Self::with_backend`] to select a backend.
pub struct Renderer {
    program: Program,
    requested: Backend,
    precise: bool,
    warnings: Vec<String>,
    cpu: std::collections::BTreeMap<String, CpuProgram>,
    #[cfg(feature = "gpu")]
    gpu: Option<synarith_gpu::GpuRenderer>,
    #[cfg(feature = "gpu")]
    compiled: std::collections::BTreeMap<String, synarith_gpu::GpuProgram>,
}
impl Renderer {
    /// Create a CPU renderer without initializing graphics.
    ///
    /// `program` must come from [`compile`] or [`load`]. Use [`Self::with_backend`]
    /// for GPU acceleration or automatic selection.
    pub fn new(program: Program) -> Self {
        Self {
            program,
            requested: Backend::Cpu,
            precise: false,
            warnings: Vec::new(),
            cpu: Default::default(),
            #[cfg(feature = "gpu")]
            gpu: None,
            #[cfg(feature = "gpu")]
            compiled: Default::default(),
        }
    }
    /// Create a renderer with the requested backend policy.
    ///
    /// # Errors
    /// Required GPU mode fails if no compatible adapter/device is available, or if
    /// the `gpu` feature was disabled. Auto records these reasons in
    /// [`Self::take_warnings`] and succeeds with CPU rendering.
    pub fn with_backend(program: Program, backend: Backend) -> Result<Self, String> {
        let mut renderer = Self::new(program);
        renderer.requested = backend;
        if backend != Backend::Cpu {
            #[cfg(feature = "gpu")]
            let result = synarith_gpu::GpuRenderer::new().map(|gpu| {
                renderer.gpu = Some(gpu);
            });
            #[cfg(not(feature = "gpu"))]
            let result: Result<(), String> = Err("GPU support was disabled at build time".into());
            if let Err(reason) = result {
                if backend == Backend::Gpu {
                    return Err(reason);
                }
                renderer
                    .warnings
                    .push(format!("GPU unavailable; using CPU: {reason}"));
            }
        }
        Ok(renderer)
    }
    /// Inspect the graph, parameter definitions, and output names without mutating it.
    pub fn program(&self) -> &Program {
        &self.program
    }
    /// Return the current executing backend, always CPU or GPU rather than Auto.
    pub fn backend(&self) -> Backend {
        if self.precise {
            return Backend::Cpu;
        }
        #[cfg(feature = "gpu")]
        if self.gpu.is_some() {
            return Backend::Gpu;
        }
        Backend::Cpu
    }
    /// Describe the current backend, including the GPU adapter name when applicable.
    pub fn backend_description(&self) -> String {
        if self.precise {
            return "CPU · f64 deep camera".into();
        }
        #[cfg(feature = "gpu")]
        if let Some(gpu) = &self.gpu {
            return format!("GPU: {}", gpu.adapter_name);
        }
        "CPU renderer".into()
    }
    /// Drain visible fallback messages after initialization and rendering.
    pub fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.warnings)
    }
    #[cfg(feature = "gpu")]
    fn gpu_region(
        &mut self,
        options: &RenderOptions,
        region: Region,
    ) -> Result<Vec<Pixel>, String> {
        let gpu = self.gpu.as_ref().ok_or("GPU unavailable")?;
        if !self.compiled.contains_key(&options.output) {
            self.compiled.insert(
                options.output.clone(),
                gpu.compile(&self.program, &options.output)?,
            );
        }
        gpu.render_region(
            &self.program,
            &self.compiled[&options.output],
            options,
            region,
        )
    }
    /// Render a rectangle of row-major float pixels using full-image coordinates.
    ///
    /// # Errors
    /// Returns validation, allocation, or cancellation errors. Required GPU mode
    /// also reports compilation, device, buffer, and dispatch-limit failures;
    /// Auto falls back to CPU and records the reason in [`Self::take_warnings`].
    pub fn render_region(
        &mut self,
        options: &RenderOptions,
        region: Region,
    ) -> Result<Vec<Pixel>, String> {
        options.validate(&self.program, region)?;
        if options.cancellation.is_cancelled() {
            return Err("render cancelled".into());
        }
        if options.precise_viewport.is_some() && self.requested == Backend::Gpu {
            return Err("Deep cameras require CPU f64 arithmetic; select Auto or CPU".into());
        }
        self.precise = options.precise_viewport.is_some();
        #[cfg(feature = "gpu")]
        if !self.precise && self.gpu.is_some() {
            match self.gpu_region(options, region) {
                Ok(pixels) => return Ok(pixels),
                Err(reason) => {
                    if self.requested == Backend::Gpu || options.cancellation.is_cancelled() {
                        return Err(reason);
                    }
                    self.gpu = None;
                    self.compiled.clear();
                    self.warnings
                        .push(format!("GPU rendering failed; using CPU: {reason}"));
                }
            }
        }
        if !self.cpu.contains_key(&options.output) {
            self.cpu.insert(
                options.output.clone(),
                CpuProgram::for_output(&self.program, &options.output)?,
            );
        }
        self.cpu[&options.output].render_region(&self.program, options, region)
    }
    /// Render a complete row-major linear premultiplied RGBA image in bounded strips.
    ///
    /// The complete returned image must fit the 1 GiB region limit. For larger
    /// exports, use [`Self::export_png`] or render individual regions.
    ///
    /// # Errors
    /// Returns validation, allocation, cancellation, or required-GPU errors as
    /// described by [`Self::render_region`].
    pub fn render(&mut self, options: &RenderOptions) -> Result<Vec<Pixel>, String> {
        options.validate(&self.program, options.full_region())?;
        let len = options.width as usize * options.height as usize;
        let mut pixels = Vec::new();
        pixels
            .try_reserve_exact(len)
            .map_err(|_| "unable to allocate image")?;
        let rows = (4_194_304 / options.width).clamp(1, 128);
        for y in (0..options.height).step_by(rows as usize) {
            pixels.extend(self.render_region(
                options,
                Region {
                    x: 0,
                    y,
                    width: options.width,
                    height: rows.min(options.height - y),
                },
            )?);
        }
        Ok(pixels)
    }
    /// Stream tiles into a PNG and atomically replace the destination only after success.
    ///
    /// Converts linear premultiplied pixels to straight sRGB RGBA8. The parent
    /// directory must exist. `overwrite = false` prevents replacement even if
    /// another writer creates the destination during rendering. On failure or
    /// cancellation, an existing destination remains intact.
    ///
    /// # Errors
    /// Returns render errors, filesystem/PNG encoding errors, or an existing-file
    /// error when overwriting is disabled.
    ///
    /// ```
    /// use synarith::{load, Renderer, RenderOptions};
    /// let program = load("syar 1;output main=rgba(0.2,0.4,0.8);")?;
    /// let mut renderer = Renderer::new(program);
    /// let path = std::path::Path::new("frame.png");
    /// # let directory = tempfile::tempdir_in(std::env::current_dir()?)?;
    /// # let path = directory.path().join(path);
    /// let options = RenderOptions { width: 64, height: 64, ..Default::default() };
    /// renderer.export_png(&options, &path, false)?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn export_png(
        &mut self,
        options: &RenderOptions,
        path: &Path,
        overwrite: bool,
    ) -> Result<(), String> {
        if path.exists() && !overwrite {
            return Err(format!("{} already exists", path.display()));
        }
        options.validate(
            &self.program,
            Region {
                x: 0,
                y: 0,
                width: options.width,
                height: 1,
            },
        )?;
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
        {
            let mut encoder = png::Encoder::new(temp.as_file_mut(), options.width, options.height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
            {
                let mut stream = writer.stream_writer().map_err(|e| e.to_string())?;
                let rows = (4_194_304 / options.width).clamp(1, 128);
                for y in (0..options.height).step_by(rows as usize) {
                    let pixels = self.render_region(
                        options,
                        Region {
                            x: 0,
                            y,
                            width: options.width,
                            height: rows.min(options.height - y),
                        },
                    )?;
                    let data = rgba8(&pixels);
                    stream.write_all(&data).map_err(|e| e.to_string())?;
                }
                stream.finish().map_err(|e| e.to_string())?;
            }
            writer.finish().map_err(|e| e.to_string())?;
        }
        temp.as_file().sync_all().map_err(|e| e.to_string())?;
        if overwrite {
            temp.persist(path).map_err(|e| e.to_string())?;
        } else {
            temp.persist_noclobber(path).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

/// Parse and compile a `.syar` document, preserving source positions in errors.
///
/// # Errors
/// Returns a diagnostic for syntax, version, name, type, cycle, constant-domain,
/// or compiler resource-limit errors. See [`parse`] and [`compile`].
pub fn load(source: &str) -> Result<Program, Diagnostic> {
    compile(&parse(source)?)
}
/// Parse a constant expression as a typed parameter value.
///
/// Colors also accept bare sRGB hex strings, such as `#80ff00aa`.
///
/// # Errors
/// Rejects nonconstant expressions, malformed values, or incompatible types.
pub fn parameter_value(ty: Type, text: &str) -> Result<Value, String> {
    let expression = if ty == Type::Color && text.starts_with('#') {
        format!("srgb(\"{text}\")")
    } else {
        text.into()
    };
    let source = format!("syar 1;param value: {ty} = {expression};output main = rgba(0,0,0);");
    let program = load(&source).map_err(|e| e.to_string())?;
    Ok(program.parameters[0].default)
}
/// Apply `NAME=VALUE` overrides to document defaults, returning a complete parameter set.
///
/// Repeated overrides for the same parameter are applied in order; the last wins.
///
/// # Errors
/// Rejects malformed assignments, unknown parameters, invalid values, and type mismatches.
///
/// ```
/// use synarith::{apply_overrides, load};
/// let program = load("syar 1;param gain:float=1.0;output main=rgba(gain,0,0);")?;
/// let values = apply_overrides(&program, &["gain=0.5".into()])?;
/// assert_eq!(values[0].data[0], 0.5);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn apply_overrides(program: &Program, overrides: &[String]) -> Result<Vec<Value>, String> {
    let mut values = program.defaults();
    for item in overrides {
        let (name, text) = item
            .split_once('=')
            .ok_or("parameter overrides must use NAME=VALUE")?;
        let index = program
            .parameter_index(name)
            .ok_or_else(|| format!("unknown parameter '{name}'"))?;
        values[index] = parameter_value(program.parameters[index].ty, text)?;
    }
    program.check_parameters(&values)?;
    Ok(values)
}
/// Compute `start + frame / fps` with an f64 intermediate and an f32 result.
///
/// Iterating `0..frame_count` avoids duplicating the endpoint of a matching loop.
///
/// # Errors
/// Rejects nonfinite start/fps, nonpositive fps, and results outside the f32 range.
///
/// ```
/// assert_eq!(synarith::animation_time(0.0, 30, 30.0)?, 1.0);
/// assert!(synarith::animation_time(0.0, 119, 30.0)? < 4.0);
/// # Ok::<(), String>(())
/// ```
pub fn animation_time(start: f32, frame: u32, fps: f32) -> Result<f32, String> {
    if !start.is_finite() || !fps.is_finite() || fps <= 0. {
        return Err("start must be finite and fps must be positive".into());
    }
    let time = start as f64 + frame as f64 / fps as f64;
    if time.abs() > f32::MAX as f64 {
        return Err("animation time exceeds numeric range".into());
    }
    Ok(time as f32)
}
