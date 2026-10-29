use synarith::*;

const TILES: &str = r##"syar 1;
use std;
tile;
param grout: color = srgb("#8a8f90");
let cells = 4.0;
let local = fract(p * cells) - vec2(0.5, 0.5);
let id = floor(p * cells);
let face = coverage(rounded_box(local, size: vec2(0.44, 0.44), radius: 0.06));
let wear = tile_fbm(p, 6.0, octaves: 4, seed: 3);
let glaze = mix(srgb("#d9eef4"), srgb("#a8cbd8"), hash(id, 5) * 0.6 + wear * 0.3);
let pits = tile_cellular(p, 24.0, seed: 9);
material {
    albedo = mix(grout, glaze, face);
    height = face * 0.004 + (wear - 0.5) * 0.0004 - smoothstep(0.2, 0.0, pits) * 0.0003;
    roughness = mix(0.9, 0.08 + wear * 0.1, face);
    metallic = 0.0;
    occlusion = 0.6 + face * 0.4;
    emissive = rgba(0, 0, 0);
}
"##;

fn options(size: u32) -> RenderOptions {
    RenderOptions {
        width: size,
        height: size,
        ..Default::default()
    }
}
fn max_difference(a: &[Pixel], b: &[Pixel]) -> f32 {
    assert_eq!(a.len(), b.len());
    a.iter()
        .flatten()
        .zip(b.iter().flatten())
        .map(|(a, b)| (a - b).abs())
        .fold(0., f32::max)
}

#[test]
fn material_channels_compile_to_typed_outputs() {
    let program = load(TILES).unwrap();
    let material = program.material.as_ref().unwrap();
    assert_eq!(material.channels, Channel::ALL.to_vec());
    assert!(program.tile);
    assert_eq!(
        program.output_kind("material.albedo").unwrap(),
        OutputKind::Color
    );
    assert_eq!(
        program.output_kind("material.normal").unwrap(),
        OutputKind::Data
    );
    assert_eq!(
        program.output_kind("material.roughness").unwrap(),
        OutputKind::Data
    );
    // `main` answers with the albedo when the document has no main output.
    assert_eq!(program.output("main"), program.output("material.albedo"));
    for (source, message) in [
        (
            "syar 1;material{albedo=rgba(1,0,0);gloss=0.5;}",
            "unknown material channel",
        ),
        (
            "syar 1;material{albedo=rgba(1,0,0);normal=rgba(0,0,1);}",
            "derived from height",
        ),
        (
            "syar 1;material{albedo=rgba(1,0,0);roughness=rgba(1,1,1);}",
            "must be float",
        ),
        ("syar 1;material{albedo=0.5;}", "must be color"),
        ("syar 1;material{height=0.5;}", "requires an albedo"),
        (
            "syar 1;material{albedo=rgba(1,0,0);albedo=rgba(0,1,0);}",
            "duplicate material channel",
        ),
        (
            "syar 1;tile;tile;output main=rgba(1,0,0);",
            "duplicate tile",
        ),
    ] {
        let error = load(source).unwrap_err().message;
        assert!(error.contains(message), "{source}: {error}");
    }
}

#[test]
fn scalar_and_vector_outputs_are_linear_data() {
    let program =
        load("syar 1;output mask=x(p);output flow=p;output main=rgba(x(p),0,0);").unwrap();
    assert_eq!(program.output_kind("mask").unwrap(), OutputKind::Data);
    assert_eq!(program.output_kind("flow").unwrap(), OutputKind::Data);
    assert_eq!(program.output_kind("main").unwrap(), OutputKind::Color);
    let value = sample(&program, "mask", [0.25, 0.5], 0., &[], None, 0.).unwrap();
    assert_eq!(value.data, [0.25, 0.25, 0.25, 1.]);
    let value = sample(&program, "flow", [0.25, 0.5], 0., &[], None, 0.).unwrap();
    assert_eq!(value.data, [0.25, 0.5, 0., 1.]);
    // Data stays linear; color is sRGB-encoded.
    assert_eq!(
        encode_rgba8(OutputKind::Data, &[[0.5, 0.5, 0.5, 1.]]),
        [128, 128, 128, 255]
    );
    assert_eq!(
        encode_rgba8(OutputKind::Color, &[[0.5, 0.5, 0.5, 1.]]),
        [188, 188, 188, 255]
    );
}

#[test]
fn fused_material_matches_separate_outputs() {
    let program = load(TILES).unwrap();
    let options = options(48);
    let fused = render_material(&program, &options, options.full_region()).unwrap();
    for (channel, pixels) in &fused.channels {
        let single = render(
            &program,
            &RenderOptions {
                output: channel.output_name(),
                ..options.clone()
            },
        )
        .unwrap();
        // The raster normal and the compiled normal differ only by rounding.
        let tolerance = if *channel == Channel::Normal {
            2e-3
        } else {
            1e-6
        };
        let difference = max_difference(&pixels.to_pixels(), &single);
        assert!(difference <= tolerance, "{channel}: {difference}");
    }
}

#[test]
fn material_regions_join_and_tiles_wrap_without_seams() {
    let program = load(TILES).unwrap();
    let options = options(32);
    let whole = render_material(&program, &options, options.full_region()).unwrap();
    let region = Region {
        x: 5,
        y: 7,
        width: 20,
        height: 9,
    };
    let part = render_material(&program, &options, region).unwrap();
    for ((channel, whole), (_, part)) in whole.channels.iter().zip(&part.channels) {
        let (whole, part) = (whole.to_pixels(), part.to_pixels());
        for y in 0..region.height {
            for x in 0..region.width {
                let a = whole[((y + region.y) * 32 + x + region.x) as usize];
                let b = part[(y * region.width + x) as usize];
                assert!(
                    max_difference(&[a], &[b]) < 1e-5,
                    "{channel} seam at {x},{y}"
                );
            }
        }
    }
    // A tile document repeats: a camera offset by one whole tile renders the same image.
    let shifted = RenderOptions {
        viewport: Some([1., -2., 1., 1.]),
        ..options.clone()
    };
    let moved = render_material(&program, &shifted, shifted.full_region()).unwrap();
    for ((channel, a), (_, b)) in whole.channels.iter().zip(&moved.channels) {
        assert!(
            max_difference(&a.to_pixels(), &b.to_pixels()) < 1e-4,
            "{channel} does not repeat"
        );
    }
    // The left edge's normals see the right edge's heights, not an empty margin.
    let normals = whole.rgba(Channel::Normal).unwrap();
    // Scalar channels are stored compactly, one value per pixel.
    assert!(matches!(
        whole.get(Channel::Height),
        Some(ChannelData::Scalar(_))
    ));
    let flat = normals.iter().filter(|n| (n[2] - 1.).abs() < 1e-6).count();
    assert!(flat < normals.len(), "normals should vary");
}

#[test]
fn renderer_material_uses_strips_and_any_backend() {
    let mut renderer = Renderer::with_backend(load(TILES).unwrap(), Backend::Auto).unwrap();
    let options = options(40);
    let image = renderer.render_material(&options).unwrap();
    eprintln!("material rendered by {}", renderer.backend_description());
    for warning in renderer.take_warnings() {
        assert!(
            std::env::var("SYAR_REQUIRE_GPU").as_deref() != Ok("1"),
            "GPU required: {warning}"
        );
        eprintln!("warning: {warning}");
    }
    assert_eq!(image.channels.len(), 7);
    for (channel, pixels) in &image.channels {
        assert_eq!(pixels.len(), 40 * 40, "{channel}");
    }
    let reference =
        render_material(&load(TILES).unwrap(), &options, options.full_region()).unwrap();
    for ((channel, a), (_, b)) in image.channels.iter().zip(&reference.channels) {
        let difference = max_difference(&a.to_pixels(), &b.to_pixels());
        assert!(
            difference <= 0.004,
            "{channel}: GPU/CPU difference {difference}"
        );
    }
}
