use crate::{Pixel, RenderOptions, Renderer, pixel_rgba8};
use image::{ExtendedColorType, ImageEncoder, codecs};
use std::{io::Write, path::Path, str::FromStr};

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
        let pixels = self.render(options)?;
        let rgb = matches!(format, ImageFormat::Jpeg | ImageFormat::Ppm);
        let mut data = Vec::new();
        data.try_reserve_exact(pixels.len() * if rgb { 3 } else { 4 })
            .map_err(|_| "unable to allocate encoding buffer")?;
        for pixel in pixels {
            let pixel = if rgb { flatten_white(pixel) } else { pixel };
            let encoded = pixel_rgba8(pixel);
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
