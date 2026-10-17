use synarith_core::*;
use synarith_cpu::*;
use synarith_gpu::GpuRenderer;

fn adapter() -> Option<GpuRenderer> {
    match GpuRenderer::new() {
        Ok(gpu) => {
            eprintln!("Executing GPU tests on {}", gpu.adapter_name);
            Some(gpu)
        }
        Err(error) => {
            assert!(
                std::env::var("SYAR_REQUIRE_GPU").as_deref() != Ok("1"),
                "GPU required: {error}"
            );
            eprintln!(
                "Skipping actual GPU execution: {error}; set SYAR_REQUIRE_GPU=1 to require it"
            );
            None
        }
    }
}
fn compare(gpu: &GpuRenderer, source: &str, label: &str, options: &RenderOptions) {
    let program = compile(&parse(source).unwrap()).unwrap_or_else(|e| panic!("{label}: {e}"));
    let compiled = gpu
        .compile(&program, &options.output)
        .unwrap_or_else(|e| panic!("{label}: {e}"));
    let actual = gpu
        .render_region(&program, &compiled, options, options.full_region())
        .unwrap();
    let expected = render(&program, options).unwrap();
    assert_eq!(actual.len(), expected.len());
    let difference = actual
        .iter()
        .flatten()
        .zip(expected.iter().flatten())
        .map(|(a, b)| {
            assert!(a.is_finite());
            (a - b).abs()
        })
        .fold(0f32, f32::max);
    assert!(
        difference <= 0.004,
        "{label}: max absolute linear RGBA difference {difference}"
    );
    let region = Region {
        x: 1,
        y: 1,
        width: options.width - 2,
        height: options.height - 2,
    };
    let tile = gpu
        .render_region(&program, &compiled, options, region)
        .unwrap();
    for y in 0..region.height {
        for x in 0..region.width {
            let whole = actual[((y + 1) * options.width + x + 1) as usize];
            let part = tile[(y * region.width + x) as usize];
            for c in 0..4 {
                assert!((whole[c] - part[c]).abs() < 1e-5, "{label}: tile seam");
            }
        }
    }
    let cancelled = RenderOptions {
        cancellation: Cancellation::default(),
        ..options.clone()
    };
    cancelled.cancellation.cancel();
    assert!(
        gpu.render_region(&program, &compiled, &cancelled, region)
            .unwrap_err()
            .contains("cancelled")
    );
}
#[test]
fn every_operation_and_bundled_document_matches_cpu() {
    let Some(gpu) = adapter() else {
        return;
    };
    for op in operations() {
        let input = |arg: &Input| match arg.default {
            Some(d) => match d.ty {
                Type::Bool => {
                    if d.data[0] != 0. {
                        "true".into()
                    } else {
                        "false".into()
                    }
                }
                Type::Color => format!(
                    "rgba({},{},{},{})",
                    d.data[0], d.data[1], d.data[2], d.data[3]
                ),
                Type::Vec2 => format!("vec2({},{})", d.data[0], d.data[1]),
                _ => format!("{}", d.data[0]),
            },
            None => match arg.ty {
                ArgType::Exact(Type::Vec2) => "p".into(),
                ArgType::Exact(Type::Color) => "rgba(0.3,0.6,0.8,0.7)".into(),
                ArgType::Exact(Type::Bool) => "true".into(),
                _ => "0.3".into(),
            },
        };
        let expression = format!(
            "{}({})",
            op.name,
            op.inputs.iter().map(input).collect::<Vec<_>>().join(",")
        );
        let expression = match op.result {
            ReturnType::Exact(Type::Color) => expression,
            ReturnType::Exact(Type::Vec2) => format!("rgba(x({expression}),y({expression}),0.2)"),
            ReturnType::Exact(Type::Bool) => {
                format!("select({expression},rgba(1,1,1),rgba(0,0,0))")
            }
            _ => format!("mix(rgba(0,0,0),rgba(1,1,1),{expression})"),
        };
        compare(
            &gpu,
            &format!("syar 1;output main={expression};"),
            op.name,
            &RenderOptions {
                width: 4,
                height: 4,
                time: 0.13,
                ..Default::default()
            },
        );
    }
    // Short orbits permit strict image comparisons; long boundary orbits amplify
    // backend rounding differences. Stable points exercise the maximum count too.
    let mandelbrot_fixture = include_str!("../../../examples/mandelbrot.syar").replace(
        "param iterations: int = 192;",
        "param iterations: int = 16;",
    );
    for (label, source) in [
        ("clouds", include_str!("../../../examples/clouds.syar")),
        ("orbit", include_str!("../../../examples/orbit.syar")),
        ("tiles", include_str!("../../../examples/tiles.syar")),
        ("paths", include_str!("../../../examples/paths.syar")),
        ("mandelbrot (16 iterations)", mandelbrot_fixture.as_str()),
        (
            "mandelbrot (1024 iterations, stable points)",
            "syar 1;output main=rgba(mandelbrot(vec2(1,0),1024),mandelbrot(vec2(-1,0),1024),mandelbrot(vec2(0,2),1024));",
        ),
        (
            "warped noise",
            "syar 1;param zoom:float=2.0;let field=fbm(p*8.0);output main=blur(normal(warp(field,vec2(noise(p*zoom),noise(p*3.0,seed:17))*0.1),epsilon:0.01),0.01);",
        ),
        (
            "negative coordinates",
            "syar 1;viewbox(-1,-1,2,2);output main=rgba(gradient_noise(p*3.0,seed:-7),periodic_noise(p*4.0,vec2(2,3)),noise(p*8.0));",
        ),
        (
            "transformed antialiasing",
            "syar 1;output main=transform(fill(star(p,5,0.25,0.1),rgba(0.4,0.8,0.7,0.5)),rotate((p-vec2(0.5,0.5))*2.0,t));",
        ),
    ] {
        compare(
            &gpu,
            source,
            label,
            &RenderOptions {
                width: 32,
                height: 24,
                time: 0.7,
                seed: Some(0xfedcba98),
                samples: 2,
                ..Default::default()
            },
        );
    }
}
