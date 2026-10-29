//! Plans for applications that dispatch Synarith shaders on their own GPU device.
use crate::{
    WORKGROUP_SIZE, material_uniform_bytes, material_wgsl, parameter_data, uniform_bytes, wgsl,
};
use synarith_core::Program;
use synarith_cpu::{MaterialArea, Pixel, Region, RenderOptions, check_material};

/// The device limits and per-dispatch budget a host plan must respect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostLimits {
    /// The device's `max_storage_buffer_binding_size`, in bytes.
    pub max_storage_buffer_binding_size: u64,
    /// The device's `max_buffer_size`, in bytes.
    pub max_buffer_size: u64,
    /// The device's `max_compute_workgroups_per_dimension`.
    pub max_compute_workgroups_per_dimension: u32,
    /// Most `vec4<f32>` values one dispatch may write. Smaller strips keep each
    /// submission short, so a host rendering beside a game stays smooth.
    pub max_strip_values: u32,
}
impl Default for HostLimits {
    /// WebGPU baseline limits, with one-million-value strips.
    fn default() -> Self {
        Self {
            max_storage_buffer_binding_size: 128 << 20,
            max_buffer_size: 256 << 20,
            max_compute_workgroups_per_dimension: 65535,
            max_strip_values: 1 << 20,
        }
    }
}

/// One dispatch of a [`HostRender`].
#[derive(Clone, Debug)]
pub struct HostStrip {
    /// The image region this strip produces.
    pub region: Region,
    /// Contents of the uniform buffer at binding 0.
    pub uniform: [u8; 80],
    /// Workgroups to dispatch along X and Y (Z is one).
    pub workgroups: [u32; 2],
    /// Bytes of binding 2 the strip writes, from offset zero; read back this many.
    pub bytes: u64,
}

/// Everything an application needs to render one output, or every channel of a
/// material, on a GPU device it owns.
///
/// Compile [`Self::shader`] (entry point `main`) with three bindings in group 0:
/// a uniform buffer (binding 0), a read-only storage buffer holding
/// [`Self::parameters`] (binding 1), and a read-write storage buffer of
/// [`Self::buffer_bytes`] (binding 2), using an explicit bind group layout: a
/// shader need not use every binding, so a derived layout can omit one. For each strip, upload its uniform,
/// dispatch its workgroups, read back its bytes, and decode them with
/// [`decode_pixels`]. Output pixels arrive in row order: append them. Material
/// strips go to [`synarith_cpu::MaterialPixels::write_region`] with the strip's region.
#[derive(Clone, Debug)]
pub struct HostRender {
    /// WGSL compute shader source.
    pub shader: String,
    /// Contents of the parameter buffer at binding 1, little-endian `vec4<f32>`s.
    pub parameters: Vec<u8>,
    /// Size of the pixel buffer at binding 2, enough for the largest strip.
    pub buffer_bytes: u64,
    /// Dispatches covering the image top to bottom.
    pub strips: Vec<HostStrip>,
}

/// Plan rendering `options.output` of `program` on a host's device.
///
/// # Errors
/// Returns invalid-option, unknown-output, and shader-generation errors, or an
/// error when even one row of the image exceeds the device limits.
pub fn host_output(
    program: &Program,
    options: &RenderOptions,
    limits: HostLimits,
) -> Result<HostRender, String> {
    options.validate(program, options.full_region())?;
    let shader = wgsl(program, &options.output)?;
    plan(program, options, limits, shader, 1, 0, |region| {
        uniform_bytes(program, options, region)
    })
}

/// Plan rendering every channel of `program`'s material on a host's device.
///
/// # Errors
/// Returns [`check_material`] and shader-generation errors, or an error when
/// even one row of the image exceeds the device limits.
pub fn host_material(
    program: &Program,
    options: &RenderOptions,
    limits: HostLimits,
) -> Result<HostRender, String> {
    let material = check_material(
        program,
        options,
        Region {
            height: 1,
            ..options.full_region()
        },
    )?;
    let shader = material_wgsl(program)?;
    let planes = material.planes().len().max(1) as u64;
    plan(
        program,
        options,
        limits,
        shader,
        planes,
        material.apron(),
        |region| material_uniform_bytes(program, options, region),
    )
}

/// Decode read-back bytes (little-endian `vec4<f32>`s) into pixels.
pub fn decode_pixels(bytes: &[u8]) -> Vec<Pixel> {
    bytes
        .as_chunks::<16>()
        .0
        .iter()
        .map(|c| {
            let f = |i: usize| f32::from_le_bytes([c[i], c[i + 1], c[i + 2], c[i + 3]]);
            [f(0), f(4), f(8), f(12)]
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn plan(
    program: &Program,
    options: &RenderOptions,
    limits: HostLimits,
    shader: String,
    planes: u64,
    apron: u32,
    uniform: impl Fn(Region) -> [u8; 80],
) -> Result<HostRender, String> {
    let width = u64::from(options.width + 2 * apron);
    let buffer_limit = limits
        .max_storage_buffer_binding_size
        .min(limits.max_buffer_size);
    // Rows per strip: within the value budget and the buffer limit, at least one.
    let values = u64::from(limits.max_strip_values).min(buffer_limit / 16);
    let rows = (values / planes / width).saturating_sub(u64::from(2 * apron));
    let rows = rows.clamp(1, u64::from(options.height)) as u32;
    let bytes_for = |height: u32| width * u64::from(height + 2 * apron) * 16 * planes;
    if bytes_for(rows) > buffer_limit {
        return Err("image too wide for this GPU's buffers".into());
    }
    let mut strips = Vec::new();
    let mut y = 0;
    while y < options.height {
        let region = Region {
            x: 0,
            y,
            width: options.width,
            height: rows.min(options.height - y),
        };
        let area = MaterialArea {
            x: -(apron as i32),
            y: y as i32 - apron as i32,
            width: options.width + 2 * apron,
            height: region.height + 2 * apron,
        };
        let workgroups = [
            area.width.div_ceil(WORKGROUP_SIZE),
            area.height.div_ceil(WORKGROUP_SIZE),
        ];
        if workgroups
            .iter()
            .any(|&n| n > limits.max_compute_workgroups_per_dimension)
        {
            return Err("image exceeds this GPU's dispatch limits".into());
        }
        strips.push(HostStrip {
            region,
            uniform: uniform(region),
            workgroups,
            bytes: bytes_for(region.height),
        });
        y += region.height;
    }
    let parameters = parameter_data(program, options)
        .iter()
        .flatten()
        .flat_map(|f| f.to_le_bytes())
        .collect();
    Ok(HostRender {
        shader,
        parameters,
        buffer_bytes: bytes_for(rows),
        strips,
    })
}
