use synarith::*;
#[cfg(feature = "gpu")]
#[test]
fn gpu_selection_overrides_outputs_and_auto_fallback() {
    let source =
        "syar 1;param tint:color=rgba(1,0,0,0.5);output main=tint;output other=rgba(x(p),y(p),t);";
    let mut renderer = match Renderer::with_backend(load(source).unwrap(), Backend::Gpu) {
        Ok(renderer) => renderer,
        Err(error) => {
            assert!(
                std::env::var("SYAR_REQUIRE_GPU").as_deref() != Ok("1"),
                "GPU required: {error}"
            );
            eprintln!("Skipping GPU facade test: {error}");
            return;
        }
    };
    assert_eq!(renderer.backend(), Backend::Gpu);
    let deep = RenderOptions {
        width: 8,
        height: 8,
        precise_viewport: Some([0.5, 0.5, 1e-10, 1e-10]),
        ..Default::default()
    };
    assert!(renderer.render(&deep).unwrap_err().contains("CPU f64"));
    let mut camera_auto = Renderer::with_backend(load(source).unwrap(), Backend::Auto).unwrap();
    camera_auto.render(&deep).unwrap();
    assert_eq!(camera_auto.backend(), Backend::Cpu);
    assert!(camera_auto.backend_description().contains("f64"));
    camera_auto
        .render(&RenderOptions {
            width: 8,
            height: 8,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(camera_auto.backend(), Backend::Gpu);
    let mut options = RenderOptions {
        width: 8,
        height: 6,
        ..Default::default()
    };
    let expected = render(renderer.program(), &options).unwrap();
    assert_eq!(renderer.render(&options).unwrap(), expected);
    options.parameters =
        apply_overrides(renderer.program(), &["tint=rgba(0,1,0,0.7)".into()]).unwrap();
    options.time = 0.3;
    for name in ["main", "other", "main"] {
        options.output = name.into();
        let expected = render(renderer.program(), &options).unwrap();
        let actual = renderer.render(&options).unwrap();
        assert!(
            actual
                .iter()
                .flatten()
                .zip(expected.iter().flatten())
                .all(|(a, b)| (a - b).abs() < 0.0001)
        );
    }
    // The default GPU dispatch limit is 65535 workgroups per dimension.
    // This valid CPU region exceeds that limit without allocating a large image.
    let wide = RenderOptions {
        width: 524_289,
        height: 1,
        ..Default::default()
    };
    assert!(
        renderer
            .render_region(&wide, wide.full_region())
            .unwrap_err()
            .contains("GPU limits")
    );
    let mut auto = Renderer::with_backend(load(source).unwrap(), Backend::Auto).unwrap();
    assert_eq!(auto.backend(), Backend::Gpu);
    let actual = auto.render_region(&wide, wide.full_region()).unwrap();
    assert_eq!(actual.len(), wide.width as usize);
    assert_eq!(auto.backend(), Backend::Cpu);
    assert!(
        auto.take_warnings()
            .iter()
            .any(|message| message.contains("GPU limits"))
    );
}
#[test]
fn cpu_backend_and_cancellation() {
    let program = load("syar 1;output main=rgba(1,0,0,0.5);").unwrap();
    let mut renderer = Renderer::with_backend(program, Backend::Cpu).unwrap();
    assert_eq!(renderer.backend(), Backend::Cpu);
    assert!(renderer.take_warnings().is_empty());
    let options = RenderOptions {
        width: 2,
        height: 2,
        ..Default::default()
    };
    assert_eq!(
        renderer.render(&options).unwrap(),
        vec![[0.5, 0., 0., 0.5]; 4]
    );
    options.cancellation.cancel();
    assert!(renderer.render(&options).unwrap_err().contains("cancelled"));
}
#[cfg(not(feature = "gpu"))]
#[test]
fn disabled_gpu_explicit_error_and_visible_auto_fallback() {
    let source = "syar 1;output main=rgba(1,1,1);";
    assert!(Renderer::with_backend(load(source).unwrap(), Backend::Gpu).is_err());
    let mut renderer = Renderer::with_backend(load(source).unwrap(), Backend::Auto).unwrap();
    assert_eq!(renderer.backend(), Backend::Cpu);
    assert!(renderer.take_warnings()[0].contains("disabled at build time"));
}
#[test]
fn animation_excludes_loop_endpoint() {
    let times: Vec<_> = (0..120)
        .map(|frame| animation_time(0., frame, 30.).unwrap())
        .collect();
    assert_eq!(times[0], 0.);
    assert!(times[119] < 4.);
    assert_eq!(animation_time(0., 120, 30.).unwrap(), 4.);
    assert!(animation_time(0., 1, 0.).is_err());
}
#[test]
fn override_types_are_checked() {
    let p =
        load("syar 1;param n:int=2;param tint:color=srgb(\"#fff000\");output main=tint;").unwrap();
    let values = apply_overrides(&p, &["n=5".into(), "tint=#80ff00aa".into()]).unwrap();
    assert_eq!(values[0].data[0], 5.);
    assert!(apply_overrides(&p, &["n=2.5".into()]).is_err());
    assert!(apply_overrides(&p, &["unknown=2".into()]).is_err());
}
