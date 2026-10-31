//! Lit previews of a document's material: the channels shaded the way a real-time
//! engine would, so relief, roughness and metal read as a surface rather than as
//! separate images.
//!
//! Shading uses the metallic-roughness model: GGX specular with Smith geometry and
//! Schlick Fresnel, Lambert diffuse, a soft ambient term darkened by occlusion, and
//! emitted light added on top, then ACES tone mapping. Normals are tangent-space
//! with +Y up the image (the material channel's convention).
use synarith::{Channel, MaterialPixels, Pixel, linear_to_srgb};

/// What the lit preview shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// The material as a flat surface seen head-on, following the camera.
    Surface,
    /// The material wrapped around a ball, four tiles around and two tall.
    Sphere,
}

/// Lit preview settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lighting {
    /// Surface or sphere.
    pub shape: Shape,
    /// Light direction around the view axis, in degrees (0 is from the right).
    pub azimuth: f32,
    /// Light height above the surface, in degrees (90 is straight on).
    pub elevation: f32,
}
impl Default for Lighting {
    fn default() -> Self {
        Self {
            shape: Shape::Surface,
            azimuth: 135.,
            elevation: 35.,
        }
    }
}
impl Lighting {
    /// Unit vector toward the light: X right, Y up, Z toward the viewer.
    pub fn direction(&self) -> [f32; 3] {
        let (az, el) = (self.azimuth.to_radians(), self.elevation.to_radians());
        [el.cos() * az.cos(), el.cos() * az.sin(), el.sin()]
    }
}

/// Edge length of the texture rendered for the sphere preview.
pub const SPHERE_TEXTURE: u32 = 512;
// Tiles around and down the ball: equirectangular, so texels stay roughly square.
const AROUND: f32 = 4.;
const DOWN: f32 = 2.;
const LIGHT: f32 = 3.2;

/// A material's channels with defaults filled in for what it doesn't declare.
struct Surface<'a> {
    pixels: &'a MaterialPixels,
}
struct Sample {
    albedo: [f32; 3],
    normal: [f32; 3],
    roughness: f32,
    metallic: f32,
    occlusion: f32,
    emissive: [f32; 3],
}
impl<'a> Surface<'a> {
    fn at(&self, i: usize) -> Sample {
        let p = self.pixels;
        let straight = |c: Pixel| {
            let a = if c[3] > 1e-6 { c[3] } else { 1. };
            [c[0] / a, c[1] / a, c[2] / a]
        };
        Sample {
            albedo: p.rgba(Channel::Albedo).map_or([0.8; 3], |v| straight(v[i])),
            normal: p.rgba(Channel::Normal).map_or([0., 0., 1.], |v| {
                normalize([v[i][0] * 2. - 1., v[i][1] * 2. - 1., v[i][2] * 2. - 1.])
            }),
            roughness: p.scalar(Channel::Roughness).map_or(0.6, |v| v[i]),
            metallic: p.scalar(Channel::Metallic).map_or(0., |v| v[i]),
            occlusion: p.scalar(Channel::Occlusion).map_or(1., |v| v[i]),
            emissive: p
                .rgba(Channel::Emissive)
                .map_or([0.; 3], |v| straight(v[i])),
        }
    }
    // Bilinear sample with wrapping, at continuous texel coordinates.
    fn sample(&self, x: f32, y: f32) -> Sample {
        let (w, h) = (self.pixels.width as i64, self.pixels.height as i64);
        let (fx, fy) = (x - 0.5, y - 0.5);
        let (x0, y0) = (fx.floor(), fy.floor());
        let (tx, ty) = (fx - x0, fy - y0);
        let index = |dx: i64, dy: i64| {
            let xi = (x0 as i64 + dx).rem_euclid(w);
            let yi = (y0 as i64 + dy).rem_euclid(h);
            (yi * w + xi) as usize
        };
        let corners = [
            (self.at(index(0, 0)), (1. - tx) * (1. - ty)),
            (self.at(index(1, 0)), tx * (1. - ty)),
            (self.at(index(0, 1)), (1. - tx) * ty),
            (self.at(index(1, 1)), tx * ty),
        ];
        let mut s = Sample {
            albedo: [0.; 3],
            normal: [0.; 3],
            roughness: 0.,
            metallic: 0.,
            occlusion: 0.,
            emissive: [0.; 3],
        };
        for (c, k) in &corners {
            for j in 0..3 {
                s.albedo[j] += c.albedo[j] * k;
                s.normal[j] += c.normal[j] * k;
                s.emissive[j] += c.emissive[j] * k;
            }
            s.roughness += c.roughness * k;
            s.metallic += c.metallic * k;
            s.occlusion += c.occlusion * k;
        }
        s.normal = normalize(s.normal);
        s
    }
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn normalize(v: [f32; 3]) -> [f32; 3] {
    let l = dot(v, v).sqrt();
    if l > 1e-8 {
        [v[0] / l, v[1] / l, v[2] / l]
    } else {
        [0., 0., 1.]
    }
}

/// Shade one sample: `n` is the world-space normal, the viewer looks along -Z.
fn shade(s: &Sample, n: [f32; 3], light: [f32; 3]) -> [f32; 3] {
    let v = [0., 0., 1.];
    let h = normalize([light[0] + v[0], light[1] + v[1], light[2] + v[2]]);
    let nl = dot(n, light).max(0.);
    let nv = dot(n, v).max(1e-4);
    let nh = dot(n, h).max(0.);
    let vh = dot(v, h).max(0.);
    let r = s.roughness.clamp(0.04, 1.);
    let a2 = (r * r) * (r * r);
    let d = a2 / (std::f32::consts::PI * ((nh * nh) * (a2 - 1.) + 1.).powi(2));
    let k = (r + 1.) * (r + 1.) / 8.;
    let g = (nl / (nl * (1. - k) + k)) * (nv / (nv * (1. - k) + k));
    let m = s.metallic.clamp(0., 1.);
    std::array::from_fn(|i| {
        let f0 = 0.04 * (1. - m) + s.albedo[i] * m;
        let f = f0 + (1. - f0) * (1. - vh).powi(5);
        let specular = d * g * f / (4. * nl * nv).max(1e-4);
        let diffuse = (1. - f) * (1. - m) * s.albedo[i] / std::f32::consts::PI;
        let ambient = (s.albedo[i] * (1. - m) * 0.16 + f0 * 0.1) * s.occlusion;
        (diffuse + specular) * nl * LIGHT + ambient + s.emissive[i]
    })
}

// ACES filmic approximation, then sRGB bytes.
fn display(c: [f32; 3]) -> [u8; 4] {
    let tone = |x: f32| {
        let x = x.max(0.);
        ((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14)).clamp(0., 1.)
    };
    let byte = |x: f32| (linear_to_srgb(tone(x)).clamp(0., 1.) * 255. + 0.5) as u8;
    [byte(c[0]), byte(c[1]), byte(c[2]), 255]
}

/// Shade a material rendered at the preview's size as a flat surface facing the
/// viewer: image right is +X and image up is +Y, like the normal channel.
pub fn shade_surface(pixels: &MaterialPixels, lighting: &Lighting) -> Vec<u8> {
    let surface = Surface { pixels };
    let light = lighting.direction();
    let len = pixels.width as usize * pixels.height as usize;
    (0..len)
        .flat_map(|i| {
            let s = surface.at(i);
            display(shade(&s, s.normal, light))
        })
        .collect()
}

/// Shade a ball `width` × `height` pixels wrapped with a material texture that
/// spans `tiles` tiles each way (rendered with [`SPHERE_TEXTURE`] pixels).
pub fn shade_sphere(
    pixels: &MaterialPixels,
    tiles: u32,
    width: u32,
    height: u32,
    lighting: &Lighting,
) -> Vec<u8> {
    let surface = Surface { pixels };
    let light = lighting.direction();
    let radius = 0.42 * width.min(height) as f32;
    let (cx, cy) = (width as f32 * 0.5, height as f32 * 0.5);
    let span = tiles.max(1) as f32;
    let mut out = Vec::with_capacity(width as usize * height as usize * 4);
    for py in 0..height {
        for px in 0..width {
            let x = (px as f32 + 0.5 - cx) / radius;
            let y = (cy - (py as f32 + 0.5)) / radius;
            let rr = x * x + y * y;
            if rr > 1. {
                // A dim vertical gradient behind the ball.
                let shade = 0.05 + 0.04 * (py as f32 / height as f32);
                out.extend_from_slice(&display([shade, shade * 1.05, shade * 1.2]));
                continue;
            }
            let z = (1. - rr).sqrt();
            let lon = x.atan2(z);
            let lat = y.asin();
            // Texture coordinates in tiles, then in the rendered span's texels.
            let u = (lon / std::f32::consts::TAU + 0.5) * AROUND / span;
            let v = (0.5 - lat / std::f32::consts::PI) * DOWN / span;
            let s = surface.sample(u * pixels.width as f32, v * pixels.height as f32);
            // Tangent (east) and bitangent (north, up the image) on the ball.
            let east = [lon.cos(), 0., -lon.sin()];
            let north = [-lat.sin() * lon.sin(), lat.cos(), -lat.sin() * lon.cos()];
            let geo = [x, y, z];
            let n = normalize([
                east[0] * s.normal[0] + north[0] * s.normal[1] + geo[0] * s.normal[2],
                east[1] * s.normal[0] + north[1] * s.normal[1] + geo[1] * s.normal[2],
                east[2] * s.normal[0] + north[2] * s.normal[1] + geo[2] * s.normal[2],
            ]);
            out.extend_from_slice(&display(shade(&s, n, light)));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use synarith::{RenderOptions, load, render_material};

    fn material(source: &str, size: u32) -> MaterialPixels {
        let program = load(source).unwrap();
        let options = RenderOptions {
            width: size,
            height: size,
            ..Default::default()
        };
        render_material(&program, &options, options.full_region()).unwrap()
    }
    fn brightness(rgba: &[u8]) -> f32 {
        rgba.chunks(4)
            .map(|p| p[..3].iter().map(|&c| c as f32).sum::<f32>())
            .sum::<f32>()
            / (rgba.len() / 4) as f32
    }

    #[test]
    fn light_from_the_front_is_brighter_than_grazing_light() {
        let flat = material(
            "syar 1;material{albedo=rgba(0.5,0.5,0.5);roughness=0.7;}",
            8,
        );
        let front = shade_surface(
            &flat,
            &Lighting {
                elevation: 90.,
                ..Default::default()
            },
        );
        let grazing = shade_surface(
            &flat,
            &Lighting {
                elevation: 5.,
                ..Default::default()
            },
        );
        assert_eq!(front.len(), 8 * 8 * 4);
        assert!(brightness(&front) > brightness(&grazing) * 1.5);
    }

    #[test]
    fn relief_catches_the_light_on_one_side() {
        // A ridge rising to the right: lit from the left, its left slope is brighter.
        let ridge = material(
            "syar 1;material{albedo=rgba(0.5,0.5,0.5);height=x(p)*0.5;roughness=0.9;}",
            8,
        );
        let left = shade_surface(
            &ridge,
            &Lighting {
                azimuth: 180.,
                elevation: 30.,
                ..Default::default()
            },
        );
        let right = shade_surface(
            &ridge,
            &Lighting {
                azimuth: 0.,
                elevation: 30.,
                ..Default::default()
            },
        );
        assert!(brightness(&left) > brightness(&right) * 1.2);
    }

    #[test]
    fn metal_tints_its_highlights_and_emission_glows() {
        let gold = "syar 1;material{albedo=rgba(1.0,0.7,0.2);roughness=0.3;metallic=1.0;}";
        let lit = shade_surface(
            &material(gold, 4),
            &Lighting {
                elevation: 90.,
                ..Default::default()
            },
        );
        assert!(lit[0] > lit[2], "gold highlights are warm: {:?}", &lit[..4]);
        let dark = "syar 1;material{albedo=rgba(0,0,0);emissive=rgba(2,0.5,0);}";
        let glow = shade_surface(
            &material(dark, 4),
            &Lighting {
                elevation: 5.,
                ..Default::default()
            },
        );
        assert!(glow[0] > 200 && glow[2] < 20, "{:?}", &glow[..4]);
    }

    #[test]
    fn the_sphere_fills_the_middle_and_leaves_a_background() {
        let tiles = material(
            "syar 1;use std;tile;material{albedo=rgba(0.6,0.4,0.3);height=tile_fbm(p,4.0)*0.01;}",
            32,
        );
        let ball = shade_sphere(
            &tiles,
            1,
            64,
            48,
            &Lighting {
                shape: Shape::Sphere,
                ..Default::default()
            },
        );
        assert_eq!(ball.len(), 64 * 48 * 4);
        let pixel = |x: usize, y: usize| &ball[(y * 64 + x) * 4..(y * 64 + x) * 4 + 3];
        let (center, corner) = (pixel(32, 24), pixel(1, 1));
        assert!(
            center.iter().map(|&c| c as u32).sum::<u32>()
                > corner.iter().map(|&c| c as u32).sum::<u32>() + 60
        );
    }
}
