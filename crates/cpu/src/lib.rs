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
//! Evaluation recursively samples the graph, including lazy transform, warp,
//! blur, and normal sources; rows are distributed through Rayon. Hash algorithms
//! and operation semantics are shared with the GPU backend, but transcendental
//! and floating-point results need not be bit-identical.
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
    options.validate(program, region)?;
    let output = program.output(&options.output)?;
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
    let mapping = Mapping::new(program.viewbox, options.width, options.height);
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
                            let value = evaluator.eval(
                                output,
                                p,
                                mapping.units_per_pixel / options.samples as f32,
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
    .eval(id, p, footprint);
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
fn imod(v: i32, p: i32) -> i32 {
    ((v % p) + p) % p
}
/// Evaluate quintic-interpolated lattice noise at a document-space point.
///
/// `gradient` selects gradient rather than value noise. Optional lattice periods
/// wrap coordinates; each component is clamped to at least one. This low-level
/// helper does not apply footprint filtering; the graph evaluator does.
pub fn noise(p: [f32; 2], seed: u32, period: Option<[i32; 2]>, gradient: bool) -> f32 {
    let cell = [p[0].floor() as i32, p[1].floor() as i32];
    let f = [p[0] - p[0].floor(), p[1] - p[1].floor()];
    let u = f.map(|v| v * v * v * (v * (v * 6. - 15.) + 10.));
    let corner = |x: i32, y: i32| {
        let px = cell[0].wrapping_add(x);
        let py = cell[1].wrapping_add(y);
        let h = if let Some(period) = period {
            hash2(imod(px, period[0].max(1)), imod(py, period[1].max(1)), seed)
        } else {
            hash2(px, py, seed)
        };
        if gradient {
            let a = h * std::f32::consts::TAU;
            a.cos() * (f[0] - x as f32) + a.sin() * (f[1] - y as f32)
        } else {
            h
        }
    };
    let a = corner(0, 0) * (1. - u[0]) + corner(1, 0) * u[0];
    let b = corner(0, 1) * (1. - u[0]) + corner(1, 1) * u[0];
    let v = a * (1. - u[1]) + b * u[1];
    if gradient { v * 0.7 + 0.5 } else { v }
}
fn cellular(p: [f32; 2], seed: u32) -> f32 {
    let cell = [p[0].floor() as i32, p[1].floor() as i32];
    let f = [p[0] - p[0].floor(), p[1] - p[1].floor()];
    let mut d = 4f32;
    for y in -1..=1 {
        for x in -1..=1 {
            let h = hash2(cell[0].wrapping_add(x), cell[1].wrapping_add(y), seed);
            let j = hash2(
                cell[0].wrapping_add(x),
                cell[1].wrapping_add(y),
                seed ^ 0x68bc21eb,
            );
            let dx = x as f32 + h - f[0];
            let dy = y as f32 + j - f[1];
            d = d.min(dx * dx + dy * dy);
        }
    }
    d.sqrt().min(1.)
}
fn segment(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let v = [b[0] - a[0], b[1] - a[1]];
    let w = [p[0] - a[0], p[1] - a[1]];
    let t = safe_div(w[0] * v[0] + w[1] * v[1], v[0] * v[0] + v[1] * v[1]).clamp(0., 1.);
    ((w[0] - v[0] * t).powi(2) + (w[1] - v[1] * t).powi(2)).sqrt()
}
fn polygon_distance(p: [f32; 2], count: i32, outer: f32, inner: Option<f32>) -> f32 {
    let vertex = |i: i32| {
        let angle = i as f32 / count as f32 * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
        let r = if i % 2 == 1 {
            inner.unwrap_or(outer)
        } else {
            outer
        };
        [r * angle.cos(), r * angle.sin()]
    };
    let mut d = f32::MAX;
    let mut inside = false;
    let mut a = vertex(count - 1);
    for i in 0..count {
        let b = vertex(i);
        d = d.min(segment(p, a, b));
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * safe_div(p[1] - a[1], b[1] - a[1]) + a[0]
        {
            inside = !inside;
        }
        a = b;
    }
    if inside { -d } else { d }
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
impl Evaluator<'_> {
    fn footprint(&self, id: usize, p: [f32; 2], fp: f32) -> f32 {
        if fp <= 0. {
            return 0.;
        }
        let center = self.eval(id, p, 0.);
        let dx = self.eval(id, [p[0] + fp, p[1]], 0.);
        let dy = self.eval(id, [p[0], p[1] + fp], 0.);
        ((dx[0] - center[0]).hypot(dx[1] - center[1]))
            .max((dy[0] - center[0]).hypot(dy[1] - center[1]))
    }
    /// Evaluate a graph node at a document-space position and sampling footprint.
    ///
    /// Callers must supply a valid node index, finite inputs, a nonnegative footprint,
    /// and a complete validated parameter slice.
    ///
    /// # Panics
    /// Panics on invalid graph/parameter indices; use [`sample`] for checked access.
    pub fn eval(&self, id: usize, p: [f32; 2], fp: f32) -> Pixel {
        let node = &self.program.nodes[id];
        let at = |index: usize| self.eval(node.inputs[index], p, fp);
        let result = match &node.kind {
            NodeKind::Constant(value) => value.data,
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
                // Sampling operations are lazy: source fields are evaluated only at requested positions.
                if matches!(op, Op::Warp | Op::Transform) {
                    let coords = at(1);
                    let transformed_fp = if fp > 0. {
                        let input = node.inputs[1];
                        let center = self.eval(input, p, 0.);
                        let dx = self.eval(input, [p[0] + fp, p[1]], 0.);
                        let dy = self.eval(input, [p[0], p[1] + fp], 0.);
                        let identity = if *op == Op::Warp { fp } else { 0. };
                        (dx[0] - center[0] + identity)
                            .hypot(dx[1] - center[1])
                            .max((dy[0] - center[0]).hypot(dy[1] - center[1] + identity))
                    } else {
                        0.
                    };
                    return self.eval(
                        node.inputs[0],
                        if *op == Op::Warp {
                            [p[0] + coords[0], p[1] + coords[1]]
                        } else {
                            [coords[0], coords[1]]
                        },
                        transformed_fp,
                    );
                }
                if *op == Op::Blur {
                    let radius = at(1)[0].max(0.);
                    let mut sum = [0.; 4];
                    for y in -1..=1 {
                        for x in -1..=1 {
                            let weight =
                                if x == 0 { 2. } else { 1. } * if y == 0 { 2. } else { 1. };
                            let v = self.eval(
                                node.inputs[0],
                                [p[0] + x as f32 * radius, p[1] + y as f32 * radius],
                                fp,
                            );
                            for i in 0..4 {
                                sum[i] += v[i] * weight / 16.;
                            }
                        }
                    }
                    return sum;
                }
                if *op == Op::Normal {
                    let strength = at(1)[0];
                    let e = at(2)[0].abs().max(1e-6);
                    let src = node.inputs[0];
                    let dx = (self.eval(src, [p[0] + e, p[1]], fp)[0]
                        - self.eval(src, [p[0] - e, p[1]], fp)[0])
                        / (2. * e);
                    let dy = (self.eval(src, [p[0], p[1] + e], fp)[0]
                        - self.eval(src, [p[0], p[1] - e], fp)[0])
                        / (2. * e);
                    let n = [-dx * strength, -dy * strength, 1.];
                    let l = (n[0] * n[0] + n[1] * n[1] + 1.).sqrt();
                    return [
                        n[0] / l * 0.5 + 0.5,
                        n[1] / l * 0.5 + 0.5,
                        n[2] / l * 0.5 + 0.5,
                        1.,
                    ];
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
                let point = [a[0], a[1]];
                use Op::*;
                match op {
                    Mandelbrot => {
                        let limit = (b[0] as i32).clamp(1, 1024);
                        let mut z = [0f32; 2];
                        let mut value = 1.;
                        for iteration in 0..limit {
                            z = [z[0] * z[0] - z[1] * z[1] + a[0], 2. * z[0] * z[1] + a[1]];
                            let squared_radius = z[0] * z[0] + z[1] * z[1];
                            if squared_radius > 256. {
                                // mu = completed iterations + 1 - log2(log(|z|)).
                                let smooth =
                                    iteration as f32 + 2. - (0.5 * squared_radius.ln()).log2();
                                value = (smooth / limit as f32).clamp(0., 0.999999);
                                break;
                            }
                        }
                        splat(value)
                    }
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
                    Noise | GradientNoise => {
                        let band = 1. - smooth(0.25, 0.75, self.footprint(node.inputs[0], p, fp));
                        splat(
                            noise(
                                point,
                                self.seed ^ (b[0] as i32 as u32),
                                None,
                                *op == GradientNoise,
                            ) * band
                                + 0.5 * (1. - band),
                        )
                    }
                    Cellular => splat(cellular(point, self.seed ^ (b[0] as i32 as u32))),
                    PeriodicNoise => splat(noise(
                        point,
                        self.seed ^ (c[0] as i32 as u32),
                        Some([b[0].round().max(1.) as i32, b[1].round().max(1.) as i32]),
                        false,
                    )),
                    Fbm | Turbulence | Ridged => {
                        let octaves = (b[0] as i32).clamp(1, 12);
                        let lacunarity = c[0].abs().clamp(0.01, 16.);
                        let gain = at(3)[0].clamp(0., 1.);
                        let seed = self.seed ^ (at(4)[0] as i32 as u32);
                        let mut freq = 1.;
                        let mut amp = 1.;
                        let mut sum = 0.;
                        let mut weight = 0.;
                        let footprint = self.footprint(node.inputs[0], p, fp);
                        for octave in 0..octaves {
                            let filter = 1. - smooth(0.25, 0.75, footprint * freq);
                            let value = noise(
                                [a[0] * freq, a[1] * freq],
                                seed.wrapping_add(octave as u32),
                                None,
                                false,
                            );
                            let value = if *op == Turbulence {
                                (value * 2. - 1.).abs()
                            } else if *op == Ridged {
                                1. - (value * 2. - 1.).abs()
                            } else {
                                value
                            };
                            sum += (value * filter + 0.5 * (1. - filter)) * amp;
                            weight += amp;
                            freq *= lacunarity;
                            amp *= gain;
                        }
                        splat(safe_div(sum, weight))
                    }
                    Linear => {
                        let d = [c[0] - b[0], c[1] - b[1]];
                        splat(safe_div(
                            (a[0] - b[0]) * d[0] + (a[1] - b[1]) * d[1],
                            d[0] * d[0] + d[1] * d[1],
                        ))
                    }
                    Radial => splat((a[0] - b[0]).hypot(a[1] - b[1]) / c[0].abs().max(1e-8)),
                    Angular => {
                        splat((a[1] - b[1]).atan2(a[0] - b[0]) / std::f32::consts::TAU + 0.5)
                    }
                    Ramp => {
                        let d = at(3);
                        let e = at(4);
                        let p1 = at(5)[0].clamp(0.0001, 0.9998);
                        let p2 = at(6)[0].clamp(p1 + 0.0001, 0.9999);
                        let x = a[0].clamp(0., 1.);
                        if x < p1 {
                            mix(b, c, x / p1)
                        } else if x < p2 {
                            mix(c, d, (x - p1) / (p2 - p1))
                        } else {
                            mix(d, e, (x - p2) / (1. - p2))
                        }
                    }
                    Circle => splat(len(a) - b[0].abs()),
                    Box | RoundedBox => {
                        let r = if *op == RoundedBox {
                            c[0].abs().min(b[0].abs().min(b[1].abs()))
                        } else {
                            0.
                        };
                        let q = [a[0].abs() - b[0].abs() + r, a[1].abs() - b[1].abs() + r];
                        splat(q[0].max(0.).hypot(q[1].max(0.)) + q[0].max(q[1]).min(0.) - r)
                    }
                    Ellipse => {
                        let rx = b[0].abs().max(1e-8);
                        let ry = b[1].abs().max(1e-8);
                        splat(((a[0] / rx).hypot(a[1] / ry) - 1.) * rx.min(ry))
                    }
                    Polygon => splat(polygon_distance(
                        point,
                        (b[0] as i32).clamp(3, 64),
                        c[0].abs(),
                        None,
                    )),
                    Star => splat(polygon_distance(
                        point,
                        (b[0] as i32).clamp(2, 32) * 2,
                        c[0].abs(),
                        Some(at(3)[0].abs()),
                    )),
                    Bezier => {
                        let d = at(3);
                        let e = at(4);
                        let mut previous = [b[0], b[1]];
                        let mut distance = f32::MAX;
                        for j in 1..=64 {
                            let u = j as f32 / 64.;
                            let v = 1. - u;
                            let next = [
                                v * v * v * b[0]
                                    + 3. * v * v * u * c[0]
                                    + 3. * v * u * u * d[0]
                                    + u * u * u * e[0],
                                v * v * v * b[1]
                                    + 3. * v * v * u * c[1]
                                    + 3. * v * u * u * d[1]
                                    + u * u * u * e[1],
                            ];
                            distance = distance.min(segment(point, previous, next));
                            previous = next;
                        }
                        splat(distance - at(5)[0].abs() * 0.5)
                    }
                    Path => {
                        let d = at(3);
                        let e = at(4);
                        let closed = at(5)[0] != 0.;
                        let tolerance = at(6)[0].abs().max(1e-8);
                        let curvature = 6.
                            * (b[0] - 2. * c[0] + d[0])
                                .hypot(b[1] - 2. * c[1] + d[1])
                                .max((c[0] - 2. * d[0] + e[0]).hypot(c[1] - 2. * d[1] + e[1]));
                        let count =
                            (curvature / (8. * tolerance)).sqrt().ceil().clamp(8., 256.) as i32;
                        let mut previous = [b[0], b[1]];
                        let mut distance = f32::MAX;
                        let mut inside = false;
                        let mut edge = |first: [f32; 2], next: [f32; 2]| {
                            distance = distance.min(segment(point, first, next));
                            if (first[1] > point[1]) != (next[1] > point[1])
                                && point[0]
                                    < (next[0] - first[0])
                                        * safe_div(point[1] - first[1], next[1] - first[1])
                                        + first[0]
                            {
                                inside = !inside;
                            }
                        };
                        for j in 1..=count {
                            let u = j as f32 / count as f32;
                            let v = 1. - u;
                            let next = [
                                v * v * v * b[0]
                                    + 3. * v * v * u * c[0]
                                    + 3. * v * u * u * d[0]
                                    + u * u * u * e[0],
                                v * v * v * b[1]
                                    + 3. * v * v * u * c[1]
                                    + 3. * v * u * u * d[1]
                                    + u * u * u * e[1],
                            ];
                            edge(previous, next);
                            previous = next;
                        }
                        if closed {
                            edge(previous, [b[0], b[1]]);
                        }
                        splat(if closed && inside {
                            -distance
                        } else {
                            distance
                        })
                    }
                    Union => splat(a[0].min(b[0])),
                    Intersect => splat(a[0].max(b[0])),
                    Subtract => splat(a[0].max(-b[0])),
                    SmoothUnion => {
                        let r = c[0].abs().max(1e-8);
                        let h = (0.5 + 0.5 * (b[0] - a[0]) / r).clamp(0., 1.);
                        splat(b[0] + (a[0] - b[0]) * h - r * h * (1. - h))
                    }
                    Fill | Stroke | Outline => {
                        let width = if fp > 0. {
                            let src = node.inputs[0];
                            let dx = self.eval(src, [p[0] + fp * 0.5, p[1]], 0.)[0]
                                - self.eval(src, [p[0] - fp * 0.5, p[1]], 0.)[0];
                            let dy = self.eval(src, [p[0], p[1] + fp * 0.5], 0.)[0]
                                - self.eval(src, [p[0], p[1] - fp * 0.5], 0.)[0];
                            (dx.abs() + dy.abs()).max(fp * 0.05)
                        } else {
                            1e-7
                        };
                        let (distance, color) = if *op == Fill {
                            (a[0], b)
                        } else if *op == Stroke {
                            (a[0].abs() - b[0].abs() * 0.5, c)
                        } else {
                            ((a[0] - b[0].abs() * 0.5).abs() - b[0].abs() * 0.5, c)
                        };
                        let coverage = 1. - smooth(-width * 0.5, width * 0.5, distance);
                        color.map(|v| v * coverage)
                    }
                    Glow => {
                        let coverage = (-a[0].max(0.) / b[0].abs().max(1e-8)).exp();
                        c.map(|v| v * coverage)
                    }
                    Translate => [a[0] - b[0], a[1] - b[1], 0., 0.],
                    Rotate => {
                        let s = b[0].sin();
                        let c = b[0].cos();
                        [c * a[0] - s * a[1], s * a[0] + c * a[1], 0., 0.]
                    }
                    Scale => [a[0] * b[0], a[1] * b[1], 0., 0.],
                    Repeat => {
                        let f = |v: f32, period: f32| {
                            let period = period.abs().max(1e-8);
                            v - period * ((v + period * 0.5) / period).floor()
                        };
                        [f(a[0], b[0]), f(a[1], b[1]), 0., 0.]
                    }
                    Mirror => [a[0].abs(), a[1].abs(), 0., 0.],
                    Polar => [len(a), a[1].atan2(a[0]), 0., 0.],
                    Over | Multiply | Screen | Overlay => {
                        let aa = a[3].clamp(0., 1.);
                        let ba = b[3].clamp(0., 1.);
                        let mut out = [0.; 4];
                        out[3] = aa + ba * (1. - aa);
                        for i in 0..3 {
                            let x = safe_div(a[i], aa);
                            let y = safe_div(b[i], ba);
                            let blend = match op {
                                Multiply => x * y,
                                Screen => x + y - x * y,
                                Overlay => {
                                    if y <= 0.5 {
                                        2. * x * y
                                    } else {
                                        1. - 2. * (1. - x) * (1. - y)
                                    }
                                }
                                _ => x,
                            };
                            out[i] = (1. - ba) * a[i] + (1. - aa) * b[i] + aa * ba * blend;
                        }
                        out
                    }
                    Mask | Opacity => a.map(|v| v * b[0].clamp(0., 1.)),
                    Invert => [a[3] - a[0], a[3] - a[1], a[3] - a[2], a[3]],
                    Grayscale => {
                        let l = a[0] * 0.2126 + a[1] * 0.7152 + a[2] * 0.0722;
                        [l, l, l, a[3]]
                    }
                    Contrast => [
                        (a[0] - a[3] * 0.5) * b[0] + a[3] * 0.5,
                        (a[1] - a[3] * 0.5) * b[0] + a[3] * 0.5,
                        (a[2] - a[3] * 0.5) * b[0] + a[3] * 0.5,
                        a[3],
                    ],
                    Saturate => {
                        let l = a[0] * 0.2126 + a[1] * 0.7152 + a[2] * 0.0722;
                        [
                            l + (a[0] - l) * b[0],
                            l + (a[1] - l) * b[0],
                            l + (a[2] - l) * b[0],
                            a[3],
                        ]
                    }
                    Loop => splat({
                        let x = safe_div(a[0], b[0].abs().max(1e-8));
                        x - x.floor()
                    }),
                    Pulse => splat(0.5 - 0.5 * (a[0] * std::f32::consts::TAU).cos()),
                    Warp | Transform | Blur | Normal => unreachable!(),
                }
            }
        };
        result.map(finite)
    }
}
