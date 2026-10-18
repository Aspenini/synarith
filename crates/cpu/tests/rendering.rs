use synarith_core::*;
use synarith_cpu::*;
fn program(s: &str) -> Program {
    compile(&parse(&format!("syar 1;use std;{s}")).unwrap()).unwrap()
}
fn image(s: &str) -> Vec<Pixel> {
    render(
        &program(s),
        &RenderOptions {
            width: 16,
            height: 16,
            ..Default::default()
        },
    )
    .unwrap()
}
#[test]
fn mandelbrot_escape_membership_iteration_bounds_and_palette_loop() {
    let source = include_str!("../../../examples/mandelbrot.syar");
    let document = parse(source).unwrap();
    let definition = &document
        .functions
        .iter()
        .find(|f| f.name == "mandelbrot")
        .unwrap();
    let definition = &source[definition.span.start..definition.span.end];
    let p = program(&format!(
        "{definition}param iterations:int=128;output main=rgba(mandelbrot(p,iterations),0,0);"
    ));
    assert_eq!(p.functions.len(), 1);
    let value = |point, iterations| {
        sample(&p, "main", point, 0., &[Value::int(iterations)], None, 0.)
            .unwrap()
            .data[0]
    };
    // Stable interior points and known escaping points of z²+c.
    for point in [[0., 0.], [-1., 0.], [-2., 0.]] {
        assert_eq!(value(point, 128), 1.);
    }
    for point in [[1., 0.], [2., 0.], [-3., 0.], [0., 2.]] {
        assert!((0. ..1.).contains(&value(point, 128)));
    }
    assert!((value([0., 1.5], 128) - value([0., -1.5], 128)).abs() < 1e-6);
    assert_eq!(value([1., 0.], 0), value([1., 0.], 1));
    assert_eq!(value([1., 0.], 2048), value([1., 0.], 1024));

    let example = compile(&document).unwrap();
    let parameters = example.defaults();
    for point in [[-1.5, 0.1], [1., 0.5], [0., 0.]] {
        let first = sample(&example, "main", point, 0., &parameters, None, 0.)
            .unwrap()
            .data;
        let looped = sample(&example, "main", point, 10., &parameters, None, 0.)
            .unwrap()
            .data;
        assert!(first.iter().zip(looped).all(|(a, b)| (a - b).abs() < 1e-5));
    }
}
#[test]
fn straight_srgb_output_and_premultiplied_float_output() {
    let p = program("output main=srgb(\"#ff800080\");");
    let opts = RenderOptions {
        width: 1,
        height: 1,
        ..Default::default()
    };
    let pixels = render(&p, &opts).unwrap();
    assert_eq!(rgba8(&pixels), [255, 128, 0, 128]);
    assert!((pixels[0][0] - 128. / 255.).abs() < 1e-6);
}
#[test]
fn compositing_matches_porter_duff() {
    let pixels = image("output main=over(rgba(1,0,0,0.5),rgba(0,0,1,0.5));");
    for p in pixels {
        assert_eq!(p, [0.5, 0., 0.25, 0.75]);
    }
}
#[test]
fn tiling_is_seam_free() {
    let p = program("output main=mix(rgba(0,0,0),rgba(1,1,1),fbm(p*9.0));");
    let opts = RenderOptions {
        width: 31,
        height: 23,
        samples: 2,
        ..Default::default()
    };
    let full = render(&p, &opts).unwrap();
    let top = render_region(
        &p,
        &opts,
        Region {
            x: 0,
            y: 0,
            width: 31,
            height: 7,
        },
    )
    .unwrap();
    let bottom = render_region(
        &p,
        &opts,
        Region {
            x: 0,
            y: 7,
            width: 31,
            height: 16,
        },
    )
    .unwrap();
    assert_eq!(full, [top, bottom].concat());
    let sub = render_region(
        &p,
        &opts,
        Region {
            x: 10,
            y: 11,
            width: 5,
            height: 4,
        },
    )
    .unwrap();
    for y in 0..4 {
        assert_eq!(
            &sub[y * 5..y * 5 + 5],
            &full[(y + 11) * 31 + 10..(y + 11) * 31 + 15]
        );
    }
}
#[test]
fn seed_and_time_are_reproducible() {
    let p = program("output main=mix(rgba(0,0,0),rgba(1,1,1),noise(p*8.0+vec2(t,0)));");
    let mut opts = RenderOptions {
        width: 32,
        height: 32,
        time: 1.2,
        seed: Some(u32::MAX),
        ..Default::default()
    };
    let first = render(&p, &opts).unwrap();
    assert_eq!(first, render(&p, &opts).unwrap());
    opts.seed = Some(1);
    assert_ne!(first, render(&p, &opts).unwrap());
    opts.seed = Some(u32::MAX);
    opts.time = 3.4;
    render(&p, &opts).unwrap();
    opts.time = 1.2;
    assert_eq!(first, render(&p, &opts).unwrap());
}
#[test]
fn seamless_noise_and_periodic_animation() {
    let p = program("output main=mix(rgba(0,0,0),rgba(1,1,1),periodic_noise(p,vec2(4,4)));");
    let a = sample(&p, "main", [0.3, 0.7], 0., &[], None, 0.).unwrap();
    let b = sample(&p, "main", [4.3, 4.7], 0., &[], None, 0.).unwrap();
    assert!((a.data[0] - b.data[0]).abs() < 1e-6);
    let p = program("output main=rgba(pulse(loop(t,4.0)),0,0);");
    assert_eq!(
        sample(&p, "main", [0., 0.], 0., &[], None, 0.).unwrap(),
        sample(&p, "main", [0., 0.], 4., &[], None, 0.).unwrap()
    );
}
#[test]
fn antialiasing_and_viewbox_letterboxing() {
    let p = program("output main=fill(circle(p-vec2(0.5,0.5),0.3));");
    let pixels = render(
        &p,
        &RenderOptions {
            width: 32,
            height: 16,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(pixels[0], [0.; 4]);
    assert!(pixels.iter().any(|p| p[3] > 0. && p[3] < 1.));
    assert_eq!(pixels[8 * 32 + 16], [1.; 4]);
}
#[test]
fn lazy_sampling_uses_procedural_sources() {
    let base = program("let paint=rgba(x(p),y(p),0);output main=sample_at(paint,p+vec2(0.2,0.3));");
    let v = sample(&base, "main", [0.1, 0.2], 0., &[], None, 0.).unwrap();
    assert!((v.data[0] - 0.3).abs() < 1e-6);
    assert_eq!(v.data[1], 0.5);
    let n = image("output main=normal(x(p),strength:1.0);");
    assert!(n.iter().all(|v| v[0] < 0.5 && v[2] > 0.8));
}
#[test]
fn cancellation_and_invalid_dimensions() {
    let p = program("output main=rgba(1,1,1);");
    let opts = RenderOptions {
        width: 0,
        ..Default::default()
    };
    assert!(render(&p, &opts).is_err());
    let opts = RenderOptions {
        samples: 9,
        ..Default::default()
    };
    assert!(render(&p, &opts).is_err());
    let opts = RenderOptions::default();
    opts.cancellation.cancel();
    assert!(render(&p, &opts).unwrap_err().contains("cancelled"));
}
#[test]
fn point_samples_do_not_depend_on_export_resolution() {
    let p = program("output main=fill(star(p-vec2(0.5,0.5),7,0.4,0.2));");
    let a = sample(&p, "main", [0.5, 0.5], 0., &[], None, 0.).unwrap();
    assert_eq!(a.data, [1.; 4]);
    let b = sample(&p, "main", [0., 0.], 0., &[], None, 0.).unwrap();
    assert_eq!(b.data, [0.; 4]);
}
