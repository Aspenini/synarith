//! # Parallel CPU reference renderer
//!
//! Evaluate compiler-produced [`Program`] graphs as continuous fields, then sample
//! them into linear premultiplied [`Pixel`] values. [`render`] allocates one image;
//! [`render_region`] permits tiled exports and [`sample`] provides checked point
//! evaluation. This crate has no graphics or window-system dependency.
//!
//! # Sampling model
//!
//! [`Mapping`] preserves the document view-box aspect ratio and centers transparent
//! padding. All regions use full-image coordinates. Deterministic supersampling
//! uses 1–8 samples per axis. Shape coverage estimates the distance footprint;
//! value, gradient, and fractal noise filter unresolved frequencies. Nonlinear
//! transforms propagate an approximate footprint, rather than promising exact
//! analytic antialiasing for every field.
//!
//! [`CpuProgram`] prepares an [`ExecutionPlan`] once for all outputs or one selected output.
//! Expensive shared expressions use registers; small trees use direct evaluation.
//! Branches and loops remain structured, and sampling creates fresh expression
//! contexts. Prepared programs can be reused across regions, time, and parameters.
//! [`sample`] and [`Evaluator`] retain recursive reference evaluation for checking
//! semantics; [`CpuProgram::sample`] uses the prepared path. CPU execution is an
//! interpreter with no JIT dependency. Rows are distributed through Rayon.
//! Hashes and operation semantics match the GPU backend; floating-point results
//! need not be bit-identical. [`RenderOptions::precise_viewport`] evaluates the same
//! scheduled graph with f64 intermediate arithmetic and coordinates on CPU. Source
//! constants and parameter values retain their compiled f32 representation.
//!
//! # Limits and cancellation
//!
//! [`RenderOptions::validate`] checks dimensions, samples, regions, parameters,
//! and output names before allocation. A region is limited to 1 GiB of float
//! pixels. Clone [`Cancellation`] and call [`Cancellation::cancel`] from another
//! thread; rendering checks it between rows and pixel batches.
//!
//! ```
//! use synarith_core::{compile, parse};
//! use synarith_cpu::{render, rgba8, RenderOptions};
//! let program = compile(&parse("syar 1;output main=rgba(1,0,0,0.5);")?)?;
//! let options = RenderOptions { width: 2, height: 2, ..Default::default() };
//! let bytes = rgba8(&render(&program, &options)?);
//! assert_eq!(&bytes[..4], &[255, 0, 0, 128]);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
#![deny(missing_docs)]
use rayon::prelude::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use synarith_core::*;

/// Linear premultiplied `[red, green, blue, alpha]` float pixel.
pub type Pixel = [f32; 4];
/// Cloneable, thread-safe cancellation flag shared by a render and its caller.
///
/// Cancellation is permanent for a token; create a new token for fresh work.
#[derive(Clone, Debug, Default)]
pub struct Cancellation(Arc<AtomicBool>);
impl Cancellation {
    /// Request cancellation for all operations sharing this token.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed)
    }
    /// Return whether cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}
/// Per-render settings shared by CPU and GPU backends.
///
/// Defaults: 512×512, time zero, document seed, one sample per axis, output
/// `main`, document parameter defaults, and a fresh cancellation token.
#[derive(Clone, Debug)]
pub struct RenderOptions {
    /// Full output width in pixels, from 1 through 1048576.
    pub width: u32,
    /// Full output height in pixels, from 1 through 1048576.
    pub height: u32,
    /// Optional document-space camera rectangle; `None` uses the document view box.
    /// Changing it resamples the field without recompilation.
    pub viewport: Option<[f32; 4]>,
    /// Optional f64 camera rectangle and CPU arithmetic. Takes precedence over
    /// `viewport`; Auto selects CPU without discarding cached GPU resources.
    /// Required GPU mode rejects this option. Constants/parameters remain f32.
    pub precise_viewport: Option<[f64; 4]>,
    /// Finite frame time in seconds.
    pub time: f32,
    /// Exact seed override; `None` uses the document seed.
    pub seed: Option<u32>,
    /// Deterministic square-grid samples per axis, from 1 through 8.
    pub samples: u32,
    /// Named color output to evaluate.
    pub output: String,
    /// Complete typed values in declaration order; empty selects document defaults.
    pub parameters: Vec<Value>,
    /// Shared cancellation token; clone it to cancel from another thread.
    pub cancellation: Cancellation,
}
impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            width: 512,
            height: 512,
            viewport: None,
            precise_viewport: None,
            time: 0.,
            seed: None,
            samples: 1,
            output: "main".into(),
            parameters: vec![],
            cancellation: Cancellation::default(),
        }
    }
}
/// A nonempty integer rectangle within the full output image.
///
/// Origins and dimensions are in pixels. Regions use the full image mapping,
/// so adjacent tiles sample the same composition without coordinate seams.
#[derive(Clone, Copy, Debug)]
pub struct Region {
    /// Left pixel offset in the full image.
    pub x: u32,
    /// Top pixel offset in the full image.
    pub y: u32,
    /// Region width in pixels.
    pub width: u32,
    /// Region height in pixels.
    pub height: u32,
}
impl RenderOptions {
    /// Check image/region bounds, settings, parameter types, and output name.
    ///
    /// # Errors
    /// Rejects empty/out-of-bounds regions, dimensions outside 1–1048576, regions
    /// above 67108864 float pixels (1 GiB), samples outside 1–8, nonfinite time,
    /// invalid parameter overrides, or unknown outputs. GPU limits are checked separately.
    pub fn validate(&self, program: &Program, region: Region) -> Result<(), String> {
        if self.width == 0 || self.height == 0 || self.width > 1_048_576 || self.height > 1_048_576
        {
            return Err("dimensions must be between 1 and 1048576".into());
        }
        if region.width == 0
            || region.height == 0
            || region
                .x
                .checked_add(region.width)
                .is_none_or(|n| n > self.width)
            || region
                .y
                .checked_add(region.height)
                .is_none_or(|n| n > self.height)
        {
            return Err("render region is outside the image".into());
        }
        if region.width as u64 * region.height as u64 > 67_108_864 {
            return Err("region exceeds 1 GiB of float pixels; render smaller tiles".into());
        }
        if !(1..=8).contains(&self.samples) {
            return Err("samples per axis must be between 1 and 8".into());
        }
        let view = self
            .precise_viewport
            .unwrap_or_else(|| self.viewport.unwrap_or(program.viewbox).map(f64::from));
        let unit = (view[2] / f64::from(self.width)).max(view[3] / f64::from(self.height));
        if !view.iter().all(|v| v.is_finite())
            || view[2] <= 0.
            || view[3] <= 0.
            || !(view[0] + view[2]).is_finite()
            || !(view[1] + view[3]).is_finite()
            || !unit.is_finite()
            || unit <= 0.
        {
            return Err(
                "viewport must be finite, positive, and representable at this resolution".into(),
            );
        }
        if self.precise_viewport.is_some() {
            let step = unit / f64::from(self.samples);
            if step * 0.5 == 0.
                || [view[0], view[1], view[0] + view[2], view[1] + view[3]]
                    .iter()
                    .any(|v| v + step == *v)
            {
                return Err("viewport sampling exceeds f64 coordinate precision; zoom out or reduce resolution".into());
            }
        }
        if self.precise_viewport.is_none()
            && Mapping::new(
                self.viewport.unwrap_or(program.viewbox),
                self.width,
                self.height,
            )
            .units_per_pixel
                <= 0.
        {
            return Err("viewport sampling width underflows f32; use precise_viewport".into());
        }
        if !self.time.is_finite() {
            return Err("time must be finite".into());
        }
        if !self.parameters.is_empty() {
            program.check_parameters(&self.parameters)?
        }
        program.output(&self.output)?;
        Ok(())
    }
    /// Return the rectangle covering the complete requested output image.
    pub fn full_region(&self) -> Region {
        Region {
            x: 0,
            y: 0,
            width: self.width,
            height: self.height,
        }
    }
}
/// Render a complete image into row-major linear premultiplied pixels.
///
/// # Errors
/// Returns validation, allocation, or cancellation errors; the full image
/// must fit the 1 GiB region limit. Use [`render_region`] for smaller tiles.
pub fn render(program: &Program, options: &RenderOptions) -> Result<Vec<Pixel>, String> {
    render_region(program, options, options.full_region())
}
/// Render a rectangle with the full image's coordinate mapping using parallel rows.
///
/// # Errors
/// Returns [`RenderOptions::validate`] errors, allocation failure, or cancellation.
/// Cancellation is checked between rows and batches of at most 64 pixels.
pub fn render_region(
    program: &Program,
    options: &RenderOptions,
    region: Region,
) -> Result<Vec<Pixel>, String> {
    CpuProgram::for_output(program, &options.output)?.render_region(program, options, region)
}

/// Prepared CPU expression schedule, reusable across regions, frames, and parameters.
///
/// Keep paired with the immutable program used to compile it. The facade renderer
/// caches this automatically. Execution uses registers for substantial shared work and
/// structured control flow; it does not JIT machine code or unroll loops.
///
/// ```
/// use synarith_core::{parse,compile};
/// use synarith_cpu::CpuProgram;
/// let program=compile(&parse("syar 1;let v=sin(x(p));output main=rgba(v+v,0,0);")?)?;
/// let cpu=CpuProgram::for_output(&program,"main")?;
/// let value=cpu.sample(&program,"main",[0.0,0.5],0.0,&[],None,0.0)?;
/// assert_eq!(value.data,[0.0,0.0,0.0,1.0]);
/// # Ok::<(),Box<dyn std::error::Error>>(())
/// ```
pub struct CpuProgram {
    plan: ExecutionPlan,
}
impl CpuProgram {
    /// Prepare all outputs of a compiler-produced program.
    ///
    /// # Errors
    /// Returns bounded schedule-allocation errors.
    pub fn compile(program: &Program) -> Result<Self, String> {
        Ok(Self {
            plan: ExecutionPlan::compile(program)?,
        })
    }
    /// Prepare one output, excluding functions used only by other outputs.
    ///
    /// # Errors
    /// Returns unknown-output or bounded schedule-allocation errors.
    pub fn for_output(program: &Program, output: &str) -> Result<Self, String> {
        Ok(Self {
            plan: ExecutionPlan::for_output(program, output)?,
        })
    }
    /// Sample an output with this prepared schedule and checked point inputs.
    ///
    /// Function locals and sampling registers belong to each call; the schedule
    /// can be reused across threads, parameter values, positions, and time.
    ///
    /// # Errors
    /// Returns invalid sample/parameter errors or an output absent from this plan.
    #[allow(clippy::too_many_arguments)]
    pub fn sample(
        &self,
        program: &Program,
        output: &str,
        p: [f32; 2],
        time: f32,
        parameters: &[Value],
        seed: Option<u32>,
        footprint: f32,
    ) -> Result<Value, String> {
        let root = program.output(output)?;
        if self.plan.block(root).is_none() {
            return Err("output not included in CPU execution plan".into());
        }
        sample_with_plan(
            program,
            output,
            p,
            time,
            parameters,
            seed,
            footprint,
            Some(&self.plan),
        )
    }
    /// Inspect backend-independent scheduling and reachability information.
    pub fn execution_plan(&self) -> &ExecutionPlan {
        &self.plan
    }
    /// Render a region with a prepared schedule. Keep `program` paired with this instance.
    ///
    /// # Errors
    /// Returns dimension, parameter, output, allocation, and cancellation errors.
    pub fn render_region(
        &self,
        program: &Program,
        options: &RenderOptions,
        region: Region,
    ) -> Result<Vec<Pixel>, String> {
        options.validate(program, region)?;
        let output = program.output(&options.output)?;
        if self.plan.block(output).is_none() {
            return Err("output not included in CPU execution plan".into());
        }
        let parameters = if options.parameters.is_empty() {
            program.defaults()
        } else {
            options.parameters.clone()
        };
        if let Some(view) = options.precise_viewport {
            return self.render_precise(program, options, region, &parameters, view);
        }
        let evaluator = Evaluator {
            program,
            parameters: &parameters,
            time: options.time,
            seed: options.seed.unwrap_or(program.seed),
        };
        let mut pixels = Vec::new();
        let len = region.width as usize * region.height as usize;
        pixels
            .try_reserve_exact(len)
            .map_err(|_| "unable to allocate render region")?;
        pixels.resize(len, [0.; 4]);
        let mapping = Mapping::new(
            options.viewport.unwrap_or(program.viewbox),
            options.width,
            options.height,
        );
        pixels
            .par_chunks_mut(region.width as usize)
            .enumerate()
            .try_for_each(|(row, pixels)| -> Result<(), String> {
                if options.cancellation.is_cancelled() {
                    return Err("render cancelled".into());
                }
                for (col, pixel) in pixels.iter_mut().enumerate() {
                    if col % 64 == 0 && options.cancellation.is_cancelled() {
                        return Err("render cancelled".into());
                    }
                    let mut total = [0.; 4];
                    for sy in 0..options.samples {
                        for sx in 0..options.samples {
                            let x = (region.x as usize + col) as f32
                                + (sx as f32 + 0.5) / options.samples as f32;
                            let y = (region.y as usize + row) as f32
                                + (sy as f32 + 0.5) / options.samples as f32;
                            let p = mapping.position(x, y);
                            if mapping.contains(p) {
                                let value = evaluator.eval_with(
                                    output,
                                    p,
                                    mapping.units_per_pixel / options.samples as f32,
                                    &[],
                                    Some(&self.plan),
                                );
                                for i in 0..4 {
                                    total[i] += value[i];
                                }
                            }
                        }
                    }
                    let count = (options.samples * options.samples) as f32;
                    *pixel = total.map(|v| finite(v / count));
                }
                Ok(())
            })?;
        Ok(pixels)
    }
    fn render_precise(
        &self,
        program: &Program,
        options: &RenderOptions,
        region: Region,
        parameters: &[Value],
        view: [f64; 4],
    ) -> Result<Vec<Pixel>, String> {
        let output = program.output(&options.output)?;
        let evaluator = Evaluator64 {
            program,
            parameters,
            time: f64::from(options.time),
            seed: options.seed.unwrap_or(program.seed),
        };
        let unit = (view[2] / f64::from(options.width)).max(view[3] / f64::from(options.height));
        // Keep offsets relative to the center until the final coordinate addition.
        let center = [view[0] + view[2] * 0.5, view[1] + view[3] * 0.5];
        let mut pixels = Vec::new();
        let len = region.width as usize * region.height as usize;
        pixels
            .try_reserve_exact(len)
            .map_err(|_| "unable to allocate render region")?;
        pixels.resize(len, [0.; 4]);
        pixels
            .par_chunks_mut(region.width as usize)
            .enumerate()
            .try_for_each(|(row, pixels)| -> Result<(), String> {
                if options.cancellation.is_cancelled() {
                    return Err("render cancelled".into());
                }
                for (col, pixel) in pixels.iter_mut().enumerate() {
                    if col % 64 == 0 && options.cancellation.is_cancelled() {
                        return Err("render cancelled".into());
                    }
                    let mut total = [0.; 4];
                    for sy in 0..options.samples {
                        for sx in 0..options.samples {
                            let x = f64::from(region.x)
                                + col as f64
                                + (f64::from(sx) + 0.5) / f64::from(options.samples);
                            let y = f64::from(region.y)
                                + row as f64
                                + (f64::from(sy) + 0.5) / f64::from(options.samples);
                            let p = [
                                center[0] + (x - f64::from(options.width) * 0.5) * unit,
                                center[1] + (y - f64::from(options.height) * 0.5) * unit,
                            ];
                            if p[0] >= view[0]
                                && p[1] >= view[1]
                                && p[0] <= view[0] + view[2]
                                && p[1] <= view[1] + view[3]
                            {
                                let value = evaluator.eval_with(
                                    output,
                                    p,
                                    unit / f64::from(options.samples),
                                    &[],
                                    Some(&self.plan),
                                );
                                for i in 0..4 {
                                    total[i] += value[i];
                                }
                            }
                        }
                    }
                    let count = f64::from(options.samples * options.samples);
                    *pixel = total.map(|v| finite((v / count) as f32));
                }
                Ok(())
            })?;
        Ok(pixels)
    }
}
/// Aspect-preserving conversion from full-image pixel coordinates to document space.
#[derive(Clone, Copy, Debug)]
pub struct Mapping {
    /// Document-space `[x, y, width, height]`.
    pub viewbox: [f32; 4],
    /// Uniform document-space distance covered by one output pixel.
    pub units_per_pixel: f32,
    /// Document-space coordinate of the full image's upper-left edge.
    pub origin: [f32; 2],
}
impl Mapping {
    /// Build an aspect-preserving mapping with centered transparent padding.
    ///
    /// Callers must supply positive dimensions and a finite, positive-size view box.
    /// Use [`RenderOptions::validate`] before constructing mappings from user input.
    pub fn new(viewbox: [f32; 4], width: u32, height: u32) -> Self {
        let unit = (viewbox[2] / width as f32).max(viewbox[3] / height as f32);
        Self {
            viewbox,
            units_per_pixel: unit,
            origin: [
                viewbox[0] + viewbox[2] * 0.5 - width as f32 * unit * 0.5,
                viewbox[1] + viewbox[3] * 0.5 - height as f32 * unit * 0.5,
            ],
        }
    }
    /// Map continuous full-image pixel coordinates into document coordinates.
    ///
    /// Pixel centers use half-integer coordinates when sampling once per axis.
    pub fn position(&self, x: f32, y: f32) -> [f32; 2] {
        [
            self.origin[0] + x * self.units_per_pixel,
            self.origin[1] + y * self.units_per_pixel,
        ]
    }
    /// Return whether a document-space point is within the inclusive view-box edges.
    pub fn contains(&self, p: [f32; 2]) -> bool {
        p[0] >= self.viewbox[0]
            && p[1] >= self.viewbox[1]
            && p[0] <= self.viewbox[0] + self.viewbox[2]
            && p[1] <= self.viewbox[1] + self.viewbox[3]
    }
}
/// Evaluate a named color field at a document-space point.
///
/// `parameters` must be a complete set; pass [`Program::defaults`] for defaults.
/// `footprint` is a nonnegative document-unit sampling width; zero requests
/// point evaluation. This function does not clip samples to the view box.
///
/// # Errors
/// Rejects unknown outputs, invalid parameters, nonfinite inputs, and negative footprints.
///
/// ```
/// use synarith_core::{parse, compile};
/// use synarith_cpu::sample;
/// let program = compile(&parse("syar 1;output main=rgba(x(p),y(p),t);")?)?;
/// let value = sample(&program, "main", [0.25, 0.5], 0.75, &program.defaults(), None, 0.0)?;
/// assert_eq!(value.data, [0.25, 0.5, 0.75, 1.0]);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn sample(
    program: &Program,
    output: &str,
    p: [f32; 2],
    time: f32,
    parameters: &[Value],
    seed: Option<u32>,
    footprint: f32,
) -> Result<Value, String> {
    sample_with_plan(program, output, p, time, parameters, seed, footprint, None)
}
#[allow(clippy::too_many_arguments)]
fn sample_with_plan(
    program: &Program,
    output: &str,
    p: [f32; 2],
    time: f32,
    parameters: &[Value],
    seed: Option<u32>,
    footprint: f32,
    plan: Option<&ExecutionPlan>,
) -> Result<Value, String> {
    program.check_parameters(parameters)?;
    if !p.iter().all(|v| v.is_finite())
        || !time.is_finite()
        || !footprint.is_finite()
        || footprint < 0.
    {
        return Err("sample inputs must be finite with nonnegative footprint".into());
    }
    let id = program.output(output)?;
    let value = Evaluator {
        program,
        parameters,
        time,
        seed: seed.unwrap_or(program.seed),
    }
    .eval_with(id, p, footprint, &[], plan);
    Ok(Value {
        ty: Type::Color,
        data: value,
    })
}
/// Convert float pixels to tightly packed straight sRGB RGBA8 bytes.
///
/// Unpremultiplies RGB and clamps exported channels; see [`pixel_rgba8`].
pub fn rgba8(pixels: &[Pixel]) -> Vec<u8> {
    pixels.iter().flat_map(|p| pixel_rgba8(*p)).collect()
}
/// Unpremultiply one linear pixel, encode RGB as sRGB, and quantize to RGBA8.
///
/// Near-zero alpha produces zero RGB. Exported channels are clamped to `[0, 255]`.
pub fn pixel_rgba8(p: Pixel) -> [u8; 4] {
    let alpha = p[3].clamp(0., 1.);
    let inv = if p[3] > 1e-8 { 1. / p[3] } else { 0. };
    [
        (linear_to_srgb(p[0] * inv).clamp(0., 1.) * 255. + 0.5) as u8,
        (linear_to_srgb(p[1] * inv).clamp(0., 1.) * 255. + 0.5) as u8,
        (linear_to_srgb(p[2] * inv).clamp(0., 1.) * 255. + 0.5) as u8,
        (alpha * 255. + 0.5) as u8,
    ]
}
/// Replace NaN and either infinity with zero, retaining other values.
pub fn finite(v: f32) -> f32 {
    if v.is_finite() { v } else { 0. }
}
mod evaluation;
/// CPU evaluator with f64 intermediate arithmetic for precise camera rendering.
/// Source constants and parameter values retain their compiled f32 values.
pub use evaluation::double::Evaluator as Evaluator64;
pub use evaluation::single::{Evaluator, hash, hash2};
