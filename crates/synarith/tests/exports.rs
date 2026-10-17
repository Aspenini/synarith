use std::path::Path;
use synarith::*;

#[test]
fn formats_preserve_dimensions_alpha_and_linear_white_compositing() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-runs");
    std::fs::create_dir_all(&root).unwrap();
    let directory = tempfile::tempdir_in(root).unwrap();
    let mut renderer = Renderer::new(load("syar 1;output main=rgba(1,0,0,0.5);").unwrap());
    for (format, extension, decoder) in [
        (ImageFormat::Jpeg, "jpg", image::ImageFormat::Jpeg),
        (ImageFormat::Bmp, "bmp", image::ImageFormat::Bmp),
        (ImageFormat::Tga, "tga", image::ImageFormat::Tga),
        (ImageFormat::Ppm, "ppm", image::ImageFormat::Pnm),
    ] {
        let options = RenderOptions {
            width: 16,
            height: 16,
            ..Default::default()
        };
        let path = directory.path().join(format!("image.{extension}"));
        renderer
            .export_image(&options, &path, format, 95, false)
            .unwrap();
        let encoded = std::fs::read(&path).unwrap();
        let decoded = image::load_from_memory_with_format(&encoded, decoder)
            .unwrap()
            .to_rgba8();
        assert_eq!(decoded.dimensions(), (16, 16));
        let pixel = decoded.get_pixel(8, 8).0;
        if matches!(format, ImageFormat::Jpeg | ImageFormat::Ppm) {
            for (actual, expected) in pixel.into_iter().zip([255u8, 188, 188, 255]) {
                assert!(actual.abs_diff(expected) <= 3, "{format:?}: {pixel:?}");
            }
        } else {
            assert_eq!(pixel, [255, 0, 0, 128], "{format:?}");
        }
        assert!(
            renderer
                .export_image(&options, &path, format, 95, false)
                .is_err()
        );
        options.cancellation.cancel();
        assert!(
            renderer
                .export_image(&options, &path, format, 95, true)
                .is_err()
        );
        assert_eq!(std::fs::read(&path).unwrap(), encoded);
    }
}

#[test]
fn export_formats_round_trip_and_reject_invalid_headers_before_allocating() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-runs");
    std::fs::create_dir_all(&root).unwrap();
    let directory = tempfile::tempdir_in(root).unwrap();
    let mut renderer = Renderer::new(load("syar 1;output main=rgba(0.2,0.4,0.6,0.5);").unwrap());
    for (format, decoder) in [
        (ImageFormat::Bmp, image::ImageFormat::Bmp),
        (ImageFormat::Tga, image::ImageFormat::Tga),
        (ImageFormat::Ppm, image::ImageFormat::Pnm),
    ] {
        let path = directory.path().join(format!("{format:?}.image"));
        let options = RenderOptions {
            width: 7,
            height: 5,
            ..Default::default()
        };
        renderer
            .export_image(&options, &path, format, 90, false)
            .unwrap();
        let bytes = std::fs::read(path).unwrap();
        let decoded = image::load_from_memory_with_format(&bytes, decoder).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (7, 5));
    }
    for format in [ImageFormat::Jpeg, ImageFormat::Tga] {
        assert!(
            renderer
                .export_image(
                    &RenderOptions {
                        width: 65536,
                        height: 1,
                        ..Default::default()
                    },
                    &directory.path().join("large.image"),
                    format,
                    90,
                    false
                )
                .unwrap_err()
                .contains("65535")
        );
    }
    assert_eq!(
        ImageFormat::from_path(Path::new("UPPER.JPG")).unwrap(),
        ImageFormat::Jpeg
    );
    assert!(ImageFormat::from_path(Path::new("unknown.webp")).is_err());
}
