use crate::{
    Channel, ChannelData, OutputKind, Pixel, RenderOptions, Renderer, pixel_data8, pixel_rgba8,
};
use image::{ExtendedColorType, ImageEncoder, codecs};
use std::{
    io::Write,
    path::{Path, PathBuf},
    str::FromStr,
};

/// Still-image export format supported by the library and `syar render`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageFormat {
    /// Lossless RGBA PNG, streamed in bounded strips.
    Png,
    /// Lossy JPEG; transparent pixels are composited over white in linear light.
    Jpeg,
    /// Lossless BMP with alpha.
    Bmp,
    /// Lossless TGA with alpha.
    Tga,
    /// Binary RGB PPM; transparent pixels are composited over white in linear light.
    Ppm,
}
impl FromStr for ImageFormat {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, String> {
        match value.to_ascii_lowercase().as_str() {
            "png" => Ok(Self::Png),
            "jpg" | "jpeg" => Ok(Self::Jpeg),
            "bmp" => Ok(Self::Bmp),
            "tga" => Ok(Self::Tga),
            "ppm" => Ok(Self::Ppm),
            _ => Err("format must be png, jpeg, bmp, tga, or ppm".into()),
        }
    }
}
impl ImageFormat {
    /// Infer a format from a case-insensitive output filename extension.
    ///
    /// # Errors
    /// Rejects missing, non-UTF-8, or unsupported extensions.
    pub fn from_path(path: &Path) -> Result<Self, String> {
        path.extension()
            .and_then(|s| s.to_str())
            .ok_or("output needs an image extension or --format")?
            .parse()
    }
}
impl Renderer {
    /// Render a still as PNG, JPEG, BMP, TGA, or PPM and persist it after successful encoding.
    ///
    /// PNG streams strips; other formats allocate a full image and use the
    /// standard 1 GiB float-pixel limit. JPEG/TGA dimensions must fit 16-bit headers.
    /// JPEG/PPM flatten transparency over white in linear light. `quality` is a
    /// JPEG quality from 1 through 100 and is ignored by other encoders.
    ///
    /// # Errors
    /// Returns invalid dimensions/quality, render, allocation, encoding, or
    /// filesystem errors. Existing destinations require `overwrite = true` and
    /// remain intact on failure or cancellation.
    pub fn export_image(
        &mut self,
        options: &RenderOptions,
        path: &Path,
        format: ImageFormat,
        quality: u8,
        overwrite: bool,
    ) -> Result<(), String> {
        if !(1..=100).contains(&quality) {
            return Err("JPEG quality must be between 1 and 100".into());
        }
        if format == ImageFormat::Png {
            return self.export_png(options, path, overwrite);
        }
        if path.exists() && !overwrite {
            return Err(format!("{} already exists", path.display()));
        }
        options.validate(self.program(), options.full_region())?;
        if matches!(format, ImageFormat::Jpeg | ImageFormat::Tga)
            && (options.width > 65535 || options.height > 65535)
        {
            return Err("JPEG and TGA dimensions must be at most 65535 per axis".into());
        }
        let kind = self.program().output_kind(&options.output)?;
        let pixels = self.render(options)?;
        let rgb = matches!(format, ImageFormat::Jpeg | ImageFormat::Ppm);
        let mut data = Vec::new();
        data.try_reserve_exact(pixels.len() * if rgb { 3 } else { 4 })
            .map_err(|_| "unable to allocate encoding buffer")?;
        for pixel in pixels {
            let encoded = match kind {
                OutputKind::Color => pixel_rgba8(if rgb { flatten_white(pixel) } else { pixel }),
                OutputKind::Data => pixel_data8(pixel),
            };
            data.extend_from_slice(&encoded[..if rgb { 3 } else { 4 }]);
        }
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
        let writer = temporary.as_file_mut();
        let result = match format {
            ImageFormat::Jpeg => codecs::jpeg::JpegEncoder::new_with_quality(writer, quality)
                .write_image(
                    &data,
                    options.width,
                    options.height,
                    ExtendedColorType::Rgb8,
                ),
            ImageFormat::Bmp => codecs::bmp::BmpEncoder::new(writer).write_image(
                &data,
                options.width,
                options.height,
                ExtendedColorType::Rgba8,
            ),
            ImageFormat::Tga => codecs::tga::TgaEncoder::new(writer).write_image(
                &data,
                options.width,
                options.height,
                ExtendedColorType::Rgba8,
            ),
            ImageFormat::Ppm => codecs::pnm::PnmEncoder::new(writer)
                .with_subtype(codecs::pnm::PnmSubtype::Pixmap(
                    codecs::pnm::SampleEncoding::Binary,
                ))
                .write_image(
                    &data,
                    options.width,
                    options.height,
                    ExtendedColorType::Rgb8,
                ),
            ImageFormat::Png => unreachable!(),
        };
        result.map_err(|e| e.to_string())?;
        if options.cancellation.is_cancelled() {
            return Err("render cancelled".into());
        }
        temporary.flush().map_err(|e| e.to_string())?;
        temporary.as_file().sync_all().map_err(|e| e.to_string())?;
        if overwrite {
            temporary.persist(path).map_err(|e| e.to_string())?;
        } else {
            temporary
                .persist_noclobber(path)
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}
fn flatten_white(pixel: Pixel) -> Pixel {
    let background = 1. - pixel[3].clamp(0., 1.);
    [
        pixel[0] + background,
        pixel[1] + background,
        pixel[2] + background,
        1.,
    ]
}

/// Files written by [`Renderer::export_material`].
#[derive(Clone, Debug)]
pub struct MaterialExport {
    /// Each channel and the PNG it was written to, in canonical channel order.
    pub files: Vec<(Channel, PathBuf)>,
    /// The height channel's `[lowest, highest]` value in document units; its
    /// 16-bit PNG maps that range to black through white.
    pub height_range: Option<[f32; 2]>,
}

impl Renderer {
    /// Render a material and write one PNG per channel into `directory`, named
    /// after the channel (`albedo.png`, `normal.png`, ...).
    ///
    /// Albedo and emissive are straight sRGB RGBA8; roughness, metallic,
    /// occlusion, and the normal are linear RGBA8. Height is 16-bit greyscale
    /// spanning [`MaterialExport::height_range`]. Every file is checked before
    /// any is written; existing files require `overwrite = true`.
    ///
    /// # Errors
    /// Returns [`Self::render_material`] errors, existing-file errors, and
    /// filesystem or encoding errors.
    pub fn export_material(
        &mut self,
        options: &RenderOptions,
        directory: &Path,
        overwrite: bool,
    ) -> Result<MaterialExport, String> {
        let material = self
            .program()
            .material
            .clone()
            .ok_or("document has no material")?;
        let files: Vec<(Channel, PathBuf)> = material
            .channels
            .iter()
            .map(|c| (*c, directory.join(format!("{}.png", c.name()))))
            .collect();
        if !overwrite && let Some((_, path)) = files.iter().find(|(_, path)| path.exists()) {
            return Err(format!("{} already exists", path.display()));
        }
        let image = self.render_material(options)?;
        std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
        let mut height_range = None;
        for ((channel, path), (_, pixels)) in files.iter().zip(&image.channels) {
            let (color, depth, data) = match pixels {
                ChannelData::Scalar(heights) if *channel == Channel::Height => {
                    let (low, high) = heights
                        .iter()
                        .fold((f32::MAX, f32::MIN), |(l, h), v| (l.min(*v), h.max(*v)));
                    height_range = Some([low, high]);
                    let span = if high > low { high - low } else { 1. };
                    let data = heights
                        .iter()
                        .flat_map(|v| {
                            ((((v - low) / span).clamp(0., 1.) * 65535. + 0.5) as u16).to_be_bytes()
                        })
                        .collect();
                    (png::ColorType::Grayscale, png::BitDepth::Sixteen, data)
                }
                _ => (
                    png::ColorType::Rgba,
                    png::BitDepth::Eight,
                    pixels.rgba8(channel.kind()),
                ),
            };
            let mut temp = tempfile::NamedTempFile::new_in(directory).map_err(|e| e.to_string())?;
            {
                let mut encoder = png::Encoder::new(temp.as_file_mut(), image.width, image.height);
                encoder.set_color(color);
                encoder.set_depth(depth);
                let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
                writer.write_image_data(&data).map_err(|e| e.to_string())?;
                writer.finish().map_err(|e| e.to_string())?;
            }
            temp.as_file().sync_all().map_err(|e| e.to_string())?;
            temp.persist(path).map_err(|e| e.to_string())?;
        }
        Ok(MaterialExport {
            files,
            height_range,
        })
    }
}
