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
//! need not be bit-identical.
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
        let view = self.viewport.unwrap_or(program.viewbox);
        if !view.iter().all(|v| v.is_finite())
            || view[2] <= 0.
            || view[3] <= 0.
            || !(view[0] + view[2]).is_finite()
            || !(view[1] + view[3]).is_finite()
            || Mapping::new(view, self.width, self.height).units_per_pixel <= 0.
        {
            return Err(
                "viewport must be finite, positive, and representable at this resolution".into(),
            );
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
fn splat(x: f32) -> Pixel {
    [x; 4]
}
fn len(v: Pixel) -> f32 {
    (v[0] * v[0] + v[1] * v[1]).sqrt()
}
fn mix(a: Pixel, b: Pixel, t: f32) -> Pixel {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}
fn smooth(a: f32, b: f32, x: f32) -> f32 {
    let t = if (b - a).abs() < 1e-10 {
        if x >= b { 1. } else { 0. }
    } else {
        ((x - a) / (b - a)).clamp(0., 1.)
    };
    t * t * (3. - 2. * t)
}
fn safe_div(a: f32, b: f32) -> f32 {
    if b.abs() > 1e-20 { a / b } else { 0. }
}

/// Fixed wrapping 32-bit avalanche hash shared with the WGSL backend.
pub fn hash(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846ca68b);
    x ^ (x >> 16)
}
/// Hash an integer lattice coordinate and seed to a value in `[0, 1)`.
pub fn hash2(x: i32, y: i32, seed: u32) -> f32 {
    ((hash((x as u32).wrapping_mul(0x9e3779b9) ^ (y as u32).wrapping_mul(0x85ebca6b) ^ seed) >> 8)
        as f32)
        / 16_777_216.
}
/// Low-level recursive CPU field evaluator.
///
/// Prefer [`sample`] for checked point sampling. Callers of this type must
/// provide a compiler-produced graph and a matching validated parameter slice.
pub struct Evaluator<'a> {
    /// Compiler-produced graph; graph indices and types must remain valid.
    pub program: &'a Program,
    /// Complete parameter values matching [`Program::parameters`].
    pub parameters: &'a [Value],
    /// Finite frame time in seconds.
    pub time: f32,
    /// Exact effective seed after applying any override.
    pub seed: u32,
}
enum Flow {
    Next,
    Break,
    Continue,
    Return(Pixel),
}
impl Evaluator<'_> {
    fn call_function(
        &self,
        index: usize,
        argument: impl Fn(usize) -> Pixel,
        p: [f32; 2],
        fp: f32,
        plan: Option<&ExecutionPlan>,
    ) -> Pixel {
        let function = &self.program.functions[index];
        let mut frame = [[0.; 4]; MAX_FUNCTION_LOCALS];
        for (slot, value) in frame.iter_mut().enumerate().take(function.parameters.len()) {
            *value = argument(slot);
        }
        match self.execute(
            &function.body,
            p,
            fp,
            &mut frame[..function.locals.len()],
            plan,
        ) {
            Flow::Return(value) => value,
            _ => [0.; 4], // Compiler validation guarantees a return on every path.
        }
    }
    fn execute(
        &self,
        body: &[Instruction],
        p: [f32; 2],
        fp: f32,
        locals: &mut [Pixel],
        plan: Option<&ExecutionPlan>,
    ) -> Flow {
        for instruction in body {
            match instruction {
                Instruction::Store { slot, value } => {
                    locals[*slot] = self.eval_with(*value, p, fp, locals, plan)
                }
                Instruction::Return(value) => {
                    return Flow::Return(self.eval_with(*value, p, fp, locals, plan));
                }
                Instruction::Break => return Flow::Break,
                Instruction::Continue => return Flow::Continue,
                Instruction::If { condition, yes, no } => {
                    let selected = if self.eval_with(*condition, p, fp, locals, plan)[0] != 0. {
                        yes
                    } else {
                        no
                    };
                    let flow = self.execute(selected, p, fp, locals, plan);
                    if !matches!(flow, Flow::Next) {
                        return flow;
                    }
                }
                Instruction::For {
                    slot,
                    start,
                    end,
                    body,
                } => {
                    for index in *start..*end {
                        locals[*slot] = [index as f32; 4];
                        match self.execute(body, p, fp, locals, plan) {
                            Flow::Break => break,
                            Flow::Return(value) => return Flow::Return(value),
                            _ => (),
                        }
                    }
                }
                Instruction::While {
                    condition,
                    limit,
                    body,
                } => {
                    for _ in 0..*limit {
                        if self.eval_with(*condition, p, fp, locals, plan)[0] == 0. {
                            break;
                        }
                        match self.execute(body, p, fp, locals, plan) {
                            Flow::Break => break,
                            Flow::Return(value) => return Flow::Return(value),
                            _ => (),
                        }
                    }
                }
            }
        }
        Flow::Next
    }
    fn footprint(
        &self,
        id: usize,
        p: [f32; 2],
        fp: f32,
        locals: &[Pixel],
        plan: Option<&ExecutionPlan>,
    ) -> f32 {
        if fp <= 0. {
            return 0.;
        }
        let center = self.eval_with(id, p, 0., locals, plan);
        let dx = self.eval_with(id, [p[0] + fp, p[1]], 0., locals, plan);
        let dy = self.eval_with(id, [p[0], p[1] + fp], 0., locals, plan);
        ((dx[0] - center[0]).hypot(dx[1] - center[1]))
            .max((dy[0] - center[0]).hypot(dy[1] - center[1]))
    }
    /// Evaluate a graph node at a document-space position and sampling footprint.
    ///
    /// Callers must supply a valid node index, finite inputs, a nonnegative footprint,
    /// and a complete validated parameter slice.
    /// Function-internal nodes require a local frame and must not be passed here;
    /// evaluate an output or function-call node instead.
    ///
    /// # Panics
    /// Panics on invalid graph/parameter indices; use [`sample`] for checked access.
    pub fn eval(&self, id: usize, p: [f32; 2], fp: f32) -> Pixel {
        self.eval_with(id, p, fp, &[], None)
    }
    fn eval_with(
        &self,
        id: usize,
        p: [f32; 2],
        fp: f32,
        locals: &[Pixel],
        plan: Option<&ExecutionPlan>,
    ) -> Pixel {
        if let Some(plan) = plan {
            let block = plan.block(id).expect("scheduled expression root");
            if block.reuse_registers {
                return self.eval_registered(block, p, fp, locals, plan);
            }
        }
        self.eval_node::<false>(id, p, fp, locals, plan, None)
    }
    #[inline(never)]
    fn eval_registered(
        &self,
        block: &ExpressionBlock,
        p: [f32; 2],
        fp: f32,
        locals: &[Pixel],
        plan: &ExecutionPlan,
    ) -> Pixel {
        let mut small = [[0.; 4]; 32];
        let mut large;
        let values: &mut [Pixel] = if block.nodes.len() <= small.len() {
            &mut small[..block.nodes.len()]
        } else {
            large = vec![[0.; 4]; block.nodes.len()];
            &mut large
        };
        for (register, instruction) in block.nodes.iter().enumerate() {
            values[register] = self.eval_node::<true>(
                instruction.node,
                p,
                fp,
                locals,
                Some(plan),
                Some((values, &instruction.inputs)),
            );
        }
        values[block.nodes.len() - 1]
    }
    fn eval_node<const REGISTERED: bool>(
        &self,
        id: usize,
        p: [f32; 2],
        fp: f32,
        locals: &[Pixel],
        plan: Option<&ExecutionPlan>,
        registers: Option<(&[Pixel], &[Option<usize>])>,
    ) -> Pixel {
        let node = &self.program.nodes[id];
        let at = |index: usize| {
            if REGISTERED {
                let (values, inputs) = registers.unwrap();
                values
                    [inputs[index].expect("lazy sources are evaluated only by sampling operations")]
            } else {
                self.eval_node::<false>(node.inputs[index], p, fp, locals, plan, None)
            }
        };
        let result = match &node.kind {
            NodeKind::Constant(value) => value.data,
            NodeKind::Local(slot) => locals[*slot],
            NodeKind::Call(index) => self.call_function(*index, at, p, fp, plan),
            NodeKind::Position => [p[0], p[1], 0., 0.],
            NodeKind::Time => splat(self.time),
            NodeKind::Parameter(index) => self.parameters[*index].data,
            NodeKind::Unary(op) => {
                let a = at(0);
                match op.as_str() {
                    "-" => a.map(|v| -v),
                    "!" => splat(if a[0] == 0. { 1. } else { 0. }),
                    _ => a,
                }
            }
            NodeKind::Binary(op) => {
                let a = at(0);
                let b = at(1);
                std::array::from_fn(|i| match op.as_str() {
                    "+" => a[i] + b[i],
                    "-" => a[i] - b[i],
                    "*" => a[i] * b[i],
                    "/" => safe_div(a[i], b[i]),
                    "%" => a[i] - b[i] * (safe_div(a[i], b[i])).floor(),
                    "<" => {
                        if a[0] < b[0] {
                            1.
                        } else {
                            0.
                        }
                    }
                    ">" => {
                        if a[0] > b[0] {
                            1.
                        } else {
                            0.
                        }
                    }
                    "<=" => {
                        if a[0] <= b[0] {
                            1.
                        } else {
                            0.
                        }
                    }
                    ">=" => {
                        if a[0] >= b[0] {
                            1.
                        } else {
                            0.
                        }
                    }
                    "==" => {
                        if a[0] == b[0] {
                            1.
                        } else {
                            0.
                        }
                    }
                    "!=" => {
                        if a[0] != b[0] {
                            1.
                        } else {
                            0.
                        }
                    }
                    "&&" => {
                        if a[0] != 0. && b[0] != 0. {
                            1.
                        } else {
                            0.
                        }
                    }
                    "||" if (a[0] != 0. || b[0] != 0.) => 1.,
                    _ => 0.,
                })
            }
            NodeKind::Builtin(op) => {
                // Source expressions stay lazy, including through expanded library groups.
                if *op == Op::SampleAt {
                    let coords = at(1);
                    let transformed_fp = self.footprint(node.inputs[1], p, fp, locals, plan);
                    return self.eval_with(
                        node.inputs[0],
                        [coords[0], coords[1]],
                        transformed_fp,
                        locals,
                        plan,
                    );
                }
                if *op == Op::PixelSize {
                    return splat(fp);
                }
                if *op == Op::Footprint {
                    let src = node.inputs[0];
                    if fp <= 0. {
                        return splat(0.);
                    }
                    if self.program.nodes[src].ty == Type::Vec2 {
                        return splat(self.footprint(src, p, fp, locals, plan));
                    }
                    let dx = self.eval_with(src, [p[0] + fp * 0.5, p[1]], 0., locals, plan)[0]
                        - self.eval_with(src, [p[0] - fp * 0.5, p[1]], 0., locals, plan)[0];
                    let dy = self.eval_with(src, [p[0], p[1] + fp * 0.5], 0., locals, plan)[0]
                        - self.eval_with(src, [p[0], p[1] - fp * 0.5], 0., locals, plan)[0];
                    return splat(dx.abs() + dy.abs());
                }
                let a = at(0);
                let b = if node.inputs.len() > 1 {
                    at(1)
                } else {
                    [0.; 4]
                };
                let c = if node.inputs.len() > 2 {
                    at(2)
                } else {
                    [0.; 4]
                };
                use Op::*;
                match op {
                    Add => std::array::from_fn(|i| a[i] + b[i]),
                    Sub => std::array::from_fn(|i| a[i] - b[i]),
                    Mul => std::array::from_fn(|i| a[i] * b[i]),
                    Div => std::array::from_fn(|i| safe_div(a[i], b[i])),
                    Mod => std::array::from_fn(|i| a[i] - b[i] * safe_div(a[i], b[i]).floor()),
                    Less => splat(if a[0] < b[0] { 1. } else { 0. }),
                    Greater => splat(if a[0] > b[0] { 1. } else { 0. }),
                    Equal => splat(if a[0] == b[0] { 1. } else { 0. }),
                    Not => splat(if a[0] == 0. { 1. } else { 0. }),
                    And => splat(if a[0] != 0. && b[0] != 0. { 1. } else { 0. }),
                    Or => splat(if a[0] != 0. || b[0] != 0. { 1. } else { 0. }),
                    ToInt => splat(a[0].clamp(-16_777_216., 16_777_216.).trunc()),
                    ToFloat => splat(a[0]),
                    Vec2 => [a[0], b[0], 0., 0.],
                    Rgba => {
                        let d = at(3)[0].clamp(0., 1.);
                        [a[0] * d, b[0] * d, c[0] * d, d]
                    }
                    X => splat(a[0]),
                    Y => splat(a[1]),
                    Red | Green | Blue => splat(safe_div(
                        a[match op {
                            Red => 0,
                            Green => 1,
                            _ => 2,
                        }],
                        a[3],
                    )),
                    Alpha => splat(a[3]),
                    Sin => a.map(f32::sin),
                    Cos => a.map(f32::cos),
                    Tan => a.map(f32::tan),
                    Abs => a.map(f32::abs),
                    Floor => a.map(f32::floor),
                    Ceil => a.map(f32::ceil),
                    Fract => a.map(|v| v - v.floor()),
                    Sqrt => a.map(|v| v.max(0.).sqrt()),
                    Exp => a.map(|v| v.min(80.).exp()),
                    Log => a.map(|v| if v > 0. { v.ln() } else { 0. }),
                    Pow => a.map(|v| finite(v.powf(b[0]))),
                    Min => std::array::from_fn(|i| a[i].min(b[i])),
                    Max => std::array::from_fn(|i| a[i].max(b[i])),
                    Clamp => a.map(|v| v.clamp(b[0].min(c[0]), b[0].max(c[0]))),
                    Step => splat(if b[0] >= a[0] { 1. } else { 0. }),
                    Smoothstep => splat(smooth(a[0], b[0], c[0])),
                    Mix => mix(a, b, c[0]),
                    Select => {
                        if a[0] != 0. {
                            b
                        } else {
                            c
                        }
                    }
                    Length => splat(len(a)),
                    Dot => splat(a[0] * b[0] + a[1] * b[1]),
                    Normalize => {
                        let l = len(a);
                        [safe_div(a[0], l), safe_div(a[1], l), 0., 0.]
                    }
                    Atan2 => splat(a[0].atan2(b[0])),
                    Hash => {
                        let seed = (self.seed ^ (b[0] as i32 as u32))
                            .wrapping_add(c[0] as i32 as u32)
                            ^ (at(3)[0] as i32 as u32).wrapping_mul(0x68bc21eb);
                        splat(hash2(
                            a[0].floor().clamp(-2147483648., 2147483520.) as i32,
                            a[1].floor().clamp(-2147483648., 2147483520.) as i32,
                            seed,
                        ))
                    }
                    Over => {
                        let aa = a[3].clamp(0., 1.);
                        [
                            a[0] + b[0] * (1. - aa),
                            a[1] + b[1] * (1. - aa),
                            a[2] + b[2] * (1. - aa),
                            aa + b[3].clamp(0., 1.) * (1. - aa),
                        ]
                    }
                    Mask | Opacity => a.map(|v| v * b[0].clamp(0., 1.)),
                    SampleAt | Footprint | PixelSize => unreachable!(),
                }
            }
        };
        result.map(finite)
    }
}
