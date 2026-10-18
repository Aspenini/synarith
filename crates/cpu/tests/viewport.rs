use synarith_core::*;
use synarith_cpu::*;
#[test]
fn camera_changes_sample_the_field_and_preserve_tile_coordinates() {
    let p = compile(&parse("syar 1;output main=rgba(x(p),y(p),0);").unwrap()).unwrap();
    let prepared = CpuProgram::compile(&p).unwrap();
    let options = RenderOptions {
        width: 2,
        height: 2,
        viewport: Some([0.25, 0.5, 0.5, 0.5]),
        ..Default::default()
    };
    let pixels = prepared
        .render_region(&p, &options, options.full_region())
        .unwrap();
    assert_eq!(pixels[0], [0.375, 0.625, 0., 1.]);
    assert_eq!(pixels[3], [0.625, 0.875, 0., 1.]);
    assert_eq!(
        prepared
            .render_region(
                &p,
                &options,
                Region {
                    x: 1,
                    y: 1,
                    width: 1,
                    height: 1
                }
            )
            .unwrap()[0],
        pixels[3]
    );
    let outside = RenderOptions {
        viewport: Some([-2., 3., 1., 1.]),
        ..options.clone()
    };
    assert_eq!(
        prepared
            .render_region(&p, &outside, outside.full_region())
            .unwrap()[0],
        [-1.75, 3.25, 0., 1.]
    );
    for view in [[0., 0., 0., 1.], [f32::NAN, 0., 1., 1.], [0., 0., 1., -1.]] {
        let invalid = RenderOptions {
            viewport: Some(view),
            ..options.clone()
        };
        assert!(invalid.validate(&p, invalid.full_region()).is_err());
    }
}

#[test]
fn deep_camera_retains_sub_float_coordinates_through_loops_and_sampling() {
    let source = "syar 1;fn field(q:vec2)->float{var v=x(q)-0.5;for i in 0..2{v*=100000.0;}return v;}output main=rgba(sample_at(field(p),p),0,0);";
    let p = compile(&parse(source).unwrap()).unwrap();
    let cpu = CpuProgram::for_output(&p, "main").unwrap();
    let options = RenderOptions {
        width: 4,
        height: 4,
        precise_viewport: Some([0.5, 0.5, 1e-10, 1e-10]),
        samples: 2,
        ..Default::default()
    };
    let pixels = cpu
        .render_region(&p, &options, options.full_region())
        .unwrap();
    // f32 collapses all four positions to 0.5; the f64 evaluator must retain
    // their differences throughout the custom function, loop, and sample_at.
    for (index, pixel) in pixels.iter().take(4).enumerate() {
        assert!((pixel[0] - (index as f32 + 0.5) / 4.).abs() < 2e-6);
    }
    let tile = cpu
        .render_region(
            &p,
            &options,
            Region {
                x: 1,
                y: 1,
                width: 2,
                height: 2,
            },
        )
        .unwrap();
    assert_eq!(tile, vec![pixels[5], pixels[6], pixels[9], pixels[10]]);
    let ordinary = RenderOptions {
        precise_viewport: None,
        viewport: Some([0.5, 0.5, 1e-10, 1e-10]),
        ..options.clone()
    };
    let collapsed = cpu
        .render_region(&p, &ordinary, ordinary.full_region())
        .unwrap();
    assert!(collapsed.iter().all(|p| p[0] == 0.));
    options.cancellation.cancel();
    assert!(
        cpu.render_region(&p, &options, options.full_region())
            .unwrap_err()
            .contains("cancelled")
    );
    for viewport in [
        [0., 0., f64::NAN, 1.],
        [0., 0., 0., 1.],
        [0., 0., f64::INFINITY, 1.],
    ] {
        let invalid = RenderOptions {
            precise_viewport: Some(viewport),
            ..Default::default()
        };
        assert!(invalid.validate(&p, invalid.full_region()).is_err());
    }
}
