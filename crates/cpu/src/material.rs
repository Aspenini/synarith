//! Fused material rendering: every channel from one evaluation per sample.
use crate::*;

/// The pixels a fused material render evaluates for a region: the region plus
/// [`Material::apron`] pixels on every side, which may lie outside the image.
///
/// The apron supplies the neighboring heights the derived normal needs, so
/// separately rendered regions still join without seams.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MaterialArea {
    /// Left pixel, in full-image coordinates; negative inside the left apron.
    pub x: i32,
    /// Top pixel, in full-image coordinates; negative inside the top apron.
    pub y: i32,
    /// Width in pixels, including both side aprons.
    pub width: u32,
    /// Height in pixels, including both top and bottom aprons.
    pub height: u32,
}
impl MaterialArea {
    /// The area evaluated for `region` of a material.
    pub fn around(material: &Material, region: Region) -> Self {
        let apron = material.apron();
        Self {
            x: region.x as i32 - apron as i32,
            y: region.y as i32 - apron as i32,
            width: region.width + 2 * apron,
            height: region.height + 2 * apron,
        }
    }
    /// Pixels in one plane of the area.
    pub fn len(&self) -> usize {
        self.width as usize * self.height as usize
    }
    /// Whether the area holds no pixels.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// One rendered material channel, row-major.
#[derive(Clone, Debug, PartialEq)]
pub enum ChannelData {
    /// Albedo and emissive (linear premultiplied RGBA) and the normal
    /// (`n * 0.5 + 0.5`, +Y up the image).
    Rgba(Vec<Pixel>),
    /// Height, roughness, metallic, or occlusion: one linear value per pixel.
    Scalar(Vec<f32>),
}
impl ChannelData {
    /// Pixels in the channel.
    pub fn len(&self) -> usize {
        match self {
            Self::Rgba(p) => p.len(),
            Self::Scalar(v) => v.len(),
        }
    }
    /// Whether the channel holds no pixels.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// The channel as RGBA pixels; a scalar becomes opaque grey, as its output renders.
    pub fn to_pixels(&self) -> Vec<Pixel> {
        match self {
            Self::Rgba(p) => p.clone(),
            Self::Scalar(v) => v.iter().map(|v| [*v, *v, *v, 1.]).collect(),
        }
    }
    /// RGBA8 bytes for storage: see [`encode_rgba8`]. Scalars are linear grey.
    pub fn rgba8(&self, kind: OutputKind) -> Vec<u8> {
        match self {
            Self::Rgba(p) => encode_rgba8(kind, p),
            Self::Scalar(v) => v
                .iter()
                .flat_map(|v| {
                    let b = (v.clamp(0., 1.) * 255. + 0.5) as u8;
                    [b, b, b, 255]
                })
                .collect(),
        }
    }
}

/// The channels of a rendered material, each row-major.
///
/// Color channels hold linear premultiplied RGBA; scalar channels hold one linear
/// value per pixel; the normal is `n * 0.5 + 0.5` with +Y up the image. Encode
/// them for storage with [`ChannelData::rgba8`] and [`Channel::kind`].
#[derive(Clone, Debug)]
pub struct MaterialPixels {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Channels in canonical [`Channel`] order.
    pub channels: Vec<(Channel, ChannelData)>,
}
impl MaterialPixels {
    /// Zeroed channels of `material` for an image of `width` × `height` pixels,
    /// ready for [`Self::write_region`].
    pub fn new(material: &Material, width: u32, height: u32) -> Self {
        let len = width as usize * height as usize;
        Self {
            width,
            height,
            channels: material
                .channels
                .iter()
                .map(|c| {
                    let data = if c.ty() == Type::Color {
                        ChannelData::Rgba(vec![[0.; 4]; len])
                    } else {
                        ChannelData::Scalar(vec![0.; len])
                    };
                    (*c, data)
                })
                .collect(),
        }
    }
    /// One channel, if the material provides it.
    pub fn get(&self, channel: Channel) -> Option<&ChannelData> {
        self.channels
            .iter()
            .find(|(c, _)| *c == channel)
            .map(|(_, data)| data)
    }
    /// A color channel (albedo, emissive, normal), if present.
    pub fn rgba(&self, channel: Channel) -> Option<&[Pixel]> {
        match self.get(channel)? {
            ChannelData::Rgba(p) => Some(p),
            ChannelData::Scalar(_) => None,
        }
    }
    /// A scalar channel (height, roughness, metallic, occlusion), if present.
    pub fn scalar(&self, channel: Channel) -> Option<&[f32]> {
        match self.get(channel)? {
            ChannelData::Scalar(v) => Some(v),
            ChannelData::Rgba(_) => None,
        }
    }
    /// glTF-style packed bytes: occlusion in red, roughness in green, metallic in
    /// blue, alpha 255, linear. A missing channel is 255 (one), so an engine's own
    /// factor for it applies unchanged; one texture serves all three slots.
    pub fn orm8(&self) -> Vec<u8> {
        let len = self.width as usize * self.height as usize;
        let [o, r, m] =
            [Channel::Occlusion, Channel::Roughness, Channel::Metallic].map(|c| self.scalar(c));
        let byte = |v: Option<&[f32]>, i: usize| {
            v.map_or(255, |v| (v[i].clamp(0., 1.) * 255. + 0.5) as u8)
        };
        (0..len)
            .flat_map(|i| [byte(o, i), byte(r, i), byte(m, i), 255])
            .collect()
    }
    /// Build the channels of `region` from fused planes: crop the apron, then
    /// derive the normal from neighboring heights.
    ///
    /// `planes` holds [`Material::planes`] in order, each [`MaterialArea::around`]
    /// the region, row-major, with the scalar channels packed one per component
    /// of the last plane: exactly what [`CpuProgram::render_material_planes`]
    /// returns and what the material compute shader writes. Hosts that dispatch
    /// that shader themselves call this, or [`Self::write_region`], on the
    /// read-back pixels.
    ///
    /// # Errors
    /// Rejects documents without a material and planes of the wrong size.
    pub fn assemble(
        program: &Program,
        options: &RenderOptions,
        region: Region,
        planes: &[Pixel],
    ) -> Result<Self, String> {
        let material = program
            .material
            .as_ref()
            .ok_or("document has no material")?;
        let mut pixels = Self::new(material, region.width, region.height);
        pixels.fill(program, options, region, [region.x, region.y], planes)?;
        Ok(pixels)
    }
    /// Write `region` of a full image from its fused planes (see [`Self::assemble`])
    /// into these channels, which must be [`Self::new`] at the image's size.
    /// Rendering strip by strip into one image this way copies each pixel once.
    ///
    /// # Errors
    /// Rejects documents without a material, planes of the wrong size, and
    /// regions outside these channels.
    pub fn write_region(
        &mut self,
        program: &Program,
        options: &RenderOptions,
        region: Region,
        planes: &[Pixel],
    ) -> Result<(), String> {
        self.fill(program, options, region, [0, 0], planes)
    }
    // Write `region` into channels whose top-left pixel is `origin` in the image.
    fn fill(
        &mut self,
        program: &Program,
        options: &RenderOptions,
        region: Region,
        origin: [u32; 2],
        planes: &[Pixel],
    ) -> Result<(), String> {
        let material = program
            .material
            .as_ref()
            .ok_or("document has no material")?;
        let order = material.planes();
        let area = MaterialArea::around(material, region);
        let n = area.len();
        if planes.len() != order.len() * n {
            return Err("material planes do not match the region".into());
        }
        let left = region.x.checked_sub(origin[0]);
        let top = region.y.checked_sub(origin[1]);
        let (Some(left), Some(top)) = (left, top) else {
            return Err("region lies outside the material pixels".into());
        };
        if left + region.width > self.width || top + region.height > self.height {
            return Err("region lies outside the material pixels".into());
        }
        let apron = material.apron() as usize;
        let stride = area.width as usize;
        let (width, left, top, dest_stride) = (
            region.width as usize,
            left as usize,
            top as usize,
            self.width as usize,
        );
        let rows = region.height as usize;
        // Where each channel lives in the planes: a whole plane, or one component.
        let mut source = std::collections::BTreeMap::new();
        for (i, plane) in order.iter().enumerate() {
            for (component, channel) in plane.iter().enumerate() {
                let packed = (channel.ty() != Type::Color).then_some(component);
                source.insert(*channel, (i, packed));
            }
        }
        let unit =
            Mapping::new(options.view(program), options.width, options.height).units_per_pixel;
        let height_at = source.get(&Channel::Height).copied();
        self.channels
            .par_iter_mut()
            .try_for_each(|(channel, data)| {
                // Destination rows of this region, in parallel.
                let span = |data_len: usize| -> std::ops::Range<usize> {
                    let start = top * dest_stride;
                    start..(start + rows * dest_stride).min(data_len)
                };
                match (data, source.get(channel)) {
                    (ChannelData::Rgba(dest), Some((plane, None))) => {
                        let plane = &planes[plane * n..(plane + 1) * n];
                        let range = span(dest.len());
                        dest[range]
                            .par_chunks_mut(dest_stride)
                            .enumerate()
                            .for_each(|(y, row)| {
                                let start = (y + apron) * stride + apron;
                                row[left..left + width]
                                    .copy_from_slice(&plane[start..start + width]);
                            });
                    }
                    (ChannelData::Scalar(dest), Some((plane, Some(c)))) => {
                        let (plane, c) = (&planes[plane * n..(plane + 1) * n], *c);
                        let range = span(dest.len());
                        dest[range]
                            .par_chunks_mut(dest_stride)
                            .enumerate()
                            .for_each(|(y, row)| {
                                let start = (y + apron) * stride + apron;
                                for (out, p) in row[left..left + width]
                                    .iter_mut()
                                    .zip(&plane[start..start + width])
                                {
                                    *out = p[c];
                                }
                            });
                    }
                    (ChannelData::Rgba(dest), None) if *channel == Channel::Normal => {
                        let (plane, c) =
                            height_at.ok_or("material normal requires a height plane")?;
                        let (heights, c) = (&planes[plane * n..(plane + 1) * n], c.unwrap_or(0));
                        let range = span(dest.len());
                        dest[range]
                            .par_chunks_mut(dest_stride)
                            .enumerate()
                            .for_each(|(y, row)| {
                                let center = (y + apron) * stride + apron;
                                for (x, normal) in row[left..left + width].iter_mut().enumerate() {
                                    let i = center + x;
                                    let hx = (heights[i + 1][c] - heights[i - 1][c]) / (2. * unit);
                                    let hy = (heights[i + stride][c] - heights[i - stride][c])
                                        / (2. * unit);
                                    *normal = surface_normal(hx, hy);
                                }
                            });
                    }
                    _ => return Err(format!("material channel {channel} has no plane")),
                }
                Ok(())
            })
    }
}

/// Encode a height gradient as a tangent-space normal, +Y up the image.
///
/// Matches the compiled `material.normal` output: `hx` and `hy` are height
/// slopes along document X (rightward) and Y (downward).
pub fn surface_normal(hx: f32, hy: f32) -> Pixel {
    let m = (hx * hx + hy * hy + 1.).sqrt();
    [
        finite(0.5 - 0.5 * hx / m),
        finite(0.5 + 0.5 * hy / m),
        finite(0.5 + 0.5 / m),
        1.,
    ]
}

/// Check that a material render is possible and within the float-pixel limit.
///
/// # Errors
/// Rejects invalid options, documents without a material, f64 cameras, and
/// areas above 1 GiB of float pixels across all planes.
pub fn check_material<'a>(
    program: &'a Program,
    options: &RenderOptions,
    region: Region,
) -> Result<&'a Material, String> {
    options.validate(program, region)?;
    let material = program
        .material
        .as_ref()
        .ok_or("document has no material")?;
    if options.precise_viewport.is_some() {
        return Err("material renders do not support f64 cameras".into());
    }
    let area = MaterialArea::around(material, region);
    if area.len() as u64 * material.planes().len() as u64 > 67_108_864 {
        return Err("material region exceeds 1 GiB of float pixels; render smaller tiles".into());
    }
    Ok(material)
}

impl CpuProgram {
    /// Prepare a fused schedule for every material plane.
    ///
    /// # Errors
    /// Rejects documents without a material and bounded scheduling-allocation errors.
    pub fn for_material(program: &Program) -> Result<Self, String> {
        let material = program
            .material
            .as_ref()
            .ok_or("document has no material")?;
        let names: Vec<String> = material
            .evaluated()
            .iter()
            .map(|c| c.output_name())
            .collect();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        Ok(Self {
            plan: ExecutionPlan::fused(program, &names)?,
        })
    }
    /// Evaluate every material plane over [`MaterialArea::around`] `region`.
    ///
    /// Shared expressions run once per sample for all channels. Material renders
    /// are not clipped to the view box: a `tile;` document repeats, and others
    /// continue past their edges. Pass the result to [`MaterialPixels::assemble`].
    ///
    /// # Errors
    /// Returns [`check_material`] errors, allocation failure, or cancellation, and
    /// an error if this program was not prepared with [`Self::for_material`].
    pub fn render_material_planes(
        &self,
        program: &Program,
        options: &RenderOptions,
        region: Region,
    ) -> Result<Vec<Pixel>, String> {
        let material = check_material(program, options, region)?;
        let block = self
            .plan
            .fused_block()
            .ok_or("CPU program was not prepared for a material")?;
        // Where each fused result goes: a whole color plane, or one component of
        // the packed scalar plane.
        let layout = material.planes();
        let planes = layout.len();
        let slots: Vec<(usize, Option<usize>)> = layout
            .iter()
            .enumerate()
            .flat_map(|(plane, channels)| {
                channels
                    .iter()
                    .enumerate()
                    .map(move |(component, channel)| {
                        let packed = channel.ty() != Type::Color;
                        (plane, packed.then_some(component))
                    })
            })
            .collect();
        let area = MaterialArea::around(material, region);
        let parameters = if options.parameters.is_empty() {
            program.defaults()
        } else {
            options.parameters.clone()
        };
        let evaluator = batch::BatchEvaluator::new(
            program,
            &parameters,
            options.time,
            options.seed.unwrap_or(program.seed),
            options.tile_span(),
            &self.plan,
        );
        let mapping = Mapping::new(options.view(program), options.width, options.height);
        let count = (options.samples * options.samples) as f32;
        let width = area.width as usize;
        let rows: Vec<Vec<Pixel>> = (0..area.height)
            .into_par_iter()
            .map_init(
                batch::Scratch::default,
                |scratch, row| -> Result<Vec<Pixel>, String> {
                    // Plane sums for every pixel of the row, pixel-major.
                    let mut sums = vec![[0f32; 4]; width * planes];
                    batch::row_batches(
                        &evaluator,
                        &mapping,
                        i64::from(area.x),
                        i64::from(area.y) + i64::from(row),
                        width,
                        options.samples,
                        false,
                        &options.cancellation,
                        |lanes, mask, owners| {
                            evaluator.eval_fused(
                                block,
                                lanes,
                                mask,
                                scratch,
                                |result, lane, value| {
                                    let (plane, component) = slots[result];
                                    let sum = &mut sums[owners[lane] * planes + plane];
                                    match component {
                                        Some(c) => sum[c] += value[0],
                                        None => {
                                            for i in 0..4 {
                                                sum[i] += value[i];
                                            }
                                        }
                                    }
                                },
                            );
                            Ok(())
                        },
                    )?;
                    Ok(sums
                        .into_iter()
                        .map(|s| s.map(|v| finite(v / count)))
                        .collect())
                },
            )
            .collect::<Result<_, _>>()?;
        let n = area.len();
        let mut out = Vec::new();
        out.try_reserve_exact(n * planes)
            .map_err(|_| "unable to allocate material planes")?;
        out.resize(n * planes, [0.; 4]);
        for (row, line) in rows.iter().enumerate() {
            for (col, pixel) in line.chunks_exact(planes).enumerate() {
                for (plane, value) in pixel.iter().enumerate() {
                    out[plane * n + row * area.width as usize + col] = *value;
                }
            }
        }
        Ok(out)
    }
    /// Render every channel of a material region, including the derived normal.
    ///
    /// # Errors
    /// See [`Self::render_material_planes`].
    pub fn render_material(
        &self,
        program: &Program,
        options: &RenderOptions,
        region: Region,
    ) -> Result<MaterialPixels, String> {
        let planes = self.render_material_planes(program, options, region)?;
        MaterialPixels::assemble(program, options, region, &planes)
    }
}

/// Render every channel of a material region on the CPU.
///
/// # Errors
/// See [`CpuProgram::render_material_planes`].
pub fn render_material(
    program: &Program,
    options: &RenderOptions,
    region: Region,
) -> Result<MaterialPixels, String> {
    CpuProgram::for_material(program)?.render_material(program, options, region)
}

/// Convert pixels to RGBA8 bytes for storage according to their kind.
///
/// Color is unpremultiplied and encoded as straight sRGB ([`pixel_rgba8`]); data
/// stays linear and straight, clamped to `[0, 1]`.
pub fn encode_rgba8(kind: OutputKind, pixels: &[Pixel]) -> Vec<u8> {
    match kind {
        OutputKind::Color => rgba8(pixels),
        OutputKind::Data => pixels.iter().flat_map(|p| pixel_data8(*p)).collect(),
    }
}
/// Quantize one linear data pixel to RGBA8 without color encoding.
pub fn pixel_data8(p: Pixel) -> [u8; 4] {
    p.map(|v| (v.clamp(0., 1.) * 255. + 0.5) as u8)
}

/// Render options for every mip level of an image, from full size down to one
/// pixel, halving each axis (rounding down, at least one). Render each level
/// from the document instead of filtering the one above: footprint-aware
/// sampling filters it properly, and normals come from that level's heights.
pub fn mip_levels(options: &RenderOptions) -> Vec<RenderOptions> {
    let mut levels = vec![options.clone()];
    let (mut width, mut height) = (options.width, options.height);
    while width > 1 || height > 1 {
        width = (width / 2).max(1);
        height = (height / 2).max(1);
        levels.push(RenderOptions {
            width,
            height,
            ..options.clone()
        });
    }
    levels
}
