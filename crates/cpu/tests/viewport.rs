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
