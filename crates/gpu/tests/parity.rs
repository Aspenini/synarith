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
    eprintln!("Comparing {label}");
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
    let fern_fixture = include_str!("../../../examples/barnsley-fern.syar")
        .replace("param points: int = 8192;", "param points: int = 256;");
    let mandelbrot_source = include_str!("../../../examples/mandelbrot.syar");
    let document = parse(mandelbrot_source).unwrap();
    let definition = document
        .functions
        .iter()
        .find(|f| f.name == "mandelbrot")
        .unwrap();
    let stable_mandelbrot = format!(
        "syar 1;use std;{}output main=rgba(mandelbrot(vec2(1,0),1024),mandelbrot(vec2(-1,0),1024),mandelbrot(vec2(0,2),1024));",
        &mandelbrot_source[definition.span.start..definition.span.end]
    );
    for (label, source) in [
        (
            "Koch snowflake",
            include_str!("../../../examples/koch-snowflake.syar"),
        ),
        (
            "Dragon curve",
            include_str!("../../../examples/dragon-curve.syar"),
        ),
        (
            "shared iterative result",
            "syar 1;fn field(v:float)->float{var x=v;for i in 0..128{x=cos(x);}return x;}let v=field(x(p));output main=rgba(v+v,v*v,v);output other=rgba(0,0,0);",
        ),
        ("Barnsley fern (256 orbit points)", fern_fixture.as_str()),
        (
            "functions and reusable groups",
            "syar 1;use std;group twice(v:float)->float{return v*2.0;}fn f(v:float)->float{var z=v;for i in 0..3{z=twice(z);}return z;}group field(q:vec2)->color{return rgba(f(x(q)),0,0);}output main=field(p);",
        ),
        (
            "volumetric space wallpaper",
            include_str!("../../../examples/deep-space.syar"),
        ),
        (
            "zero/max bounds and branch returns",
            "syar 1;use std;fn f(v:float)->float{if v>0.4{return 0.7;}else{return 0.2;}}fn g()->float{var v=0;for i in 0..4096{v+=1;}while true limit 0{v+=1;}return float(v)/4096.0;}output main=rgba(f(x(p)),g(),0);",
        ),
        (
            "user Julia algorithm",
            include_str!("../../../examples/algorithms.syar"),
        ),
        (
            "structured loops and early returns",
            r#"syar 1;use std;
fn square(a:float)->float{return a*a;}
fn sum(v:float)->float{var result=0.0;for i in 0..8{if i==6{break;}if i%2==0{continue;}result+=square(v+float(i)*0.01);}return result;}
fn march(v:float)->float{var x=v;while x<0.9 limit 12 {x+=0.07;if x>0.7 {return x;}}return x;}
fn nested()->float{var v=0.0;for a in 0..3{for b in 0..4{if b==2{break;}v+=float(a)*0.01;}}return v;}
output main=rgba(sum(x(p)),march(y(p)),nested());"#,
        ),
        (
            "local frame through source sampling",
            "syar 1;use std;fn field(q:vec2)->float{var v=0.0;for i in 0..3{let offset=float(i)*0.2;v+=noise(q+vec2(offset,0),seed:7);}return v/3.0;}output main=blur(normal(sample_at(field(p),p*2.0),epsilon:0.01),0.01);",
        ),
        ("clouds", include_str!("../../../examples/clouds.syar")),
        ("orbit", include_str!("../../../examples/orbit.syar")),
        ("tiles", include_str!("../../../examples/tiles.syar")),
        ("paths", include_str!("../../../examples/paths.syar")),
        ("mandelbrot (16 iterations)", mandelbrot_fixture.as_str()),
        (
            "mandelbrot (1024 iterations, stable points)",
            stable_mandelbrot.as_str(),
        ),
        (
            "warped noise",
            "syar 1;use std;param zoom:float=2.0;let field=fbm(p*8.0);output main=blur(normal(warp(field,vec2(noise(p*zoom),noise(p*3.0,seed:17))*0.1),epsilon:0.01),0.01);",
        ),
        (
            "negative coordinates",
            "syar 1;use std;viewbox(-1,-1,2,2);output main=rgba(gradient_noise(p*3.0,seed:-7),periodic_noise(p*4.0,vec2(2,3)),noise(p*8.0));",
        ),
        (
            "transformed antialiasing",
            "syar 1;use std;output main=sample_at(fill(star(p,5,0.25,0.1),rgba(0.4,0.8,0.7,0.5)),rotate((p-vec2(0.5,0.5))*2.0,t));",
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
    compare(
        &gpu,
        &fern_fixture,
        "Barnsley fern transparent output",
        &RenderOptions {
            width: 12,
            height: 21,
            output: "fern".into(),
            ..Default::default()
        },
    );
}

#[test]
fn every_public_source_library_algorithm_matches_cpu() {
    let Some(gpu) = adapter() else {
        return;
    };
    let library = parse(standard_library_source()).unwrap();
    for (name, args, defaults, ty) in library
        .functions
        .iter()
        .map(|f| (&f.name, &f.args, &f.defaults, f.ty))
        .chain(
            library
                .groups
                .iter()
                .map(|g| (&g.name, &g.args, &g.defaults, g.ty)),
        )
    {
        if name.starts_with("std_") {
            continue;
        }
        let arguments = args
            .iter()
            .zip(defaults)
            .map(|((_, ty), default)| {
                if let Some(e) = default {
                    library.source[e.span.start..e.span.end].to_owned()
                } else {
                    match ty {
                        Type::Vec2 => "p-vec2(0.5,0.5)",
                        Type::Color => "rgba(x(p),y(p),0.3,0.6)",
                        Type::Bool => "true",
                        Type::Int => "5",
                        Type::Float => "x(p)",
                    }
                    .to_owned()
                }
            })
            .collect::<Vec<_>>()
            .join(",");
        let call = format!("{name}({arguments})");
        let output = match ty {
            Type::Color => call,
            Type::Vec2 => format!("rgba(x({call}),y({call}),0)"),
            _ => format!("mix(rgba(0,0,0),rgba(1,1,1),{call})"),
        };

        compare(
            &gpu,
            &format!("syar 1;use std;seed 4294967295;output main={output};"),
            name,
            &RenderOptions {
                width: 4,
                height: 4,
                samples: 2,
                ..Default::default()
            },
        );
    }
}

#[test]
fn camera_uniforms_change_without_recompiling_the_program() {
    let Some(gpu) = adapter() else {
        return;
    };
    let program = compile(&parse("syar 1;output main=rgba(x(p),y(p),0);").unwrap()).unwrap();
    let compiled = gpu.compile(&program, "main").unwrap();
    for view in [[0.25, 0.5, 0.5, 0.5], [-2.0, 3.0, 1.0, 1.0]] {
        let options = RenderOptions {
            width: 16,
            height: 12,
            viewport: Some(view),
            samples: 2,
            ..Default::default()
        };
        let actual = gpu
            .render_region(&program, &compiled, &options, options.full_region())
            .unwrap();
        let expected = render(&program, &options).unwrap();
        assert!(
            actual
                .iter()
                .flatten()
                .zip(expected.iter().flatten())
                .all(|(a, b)| (a - b).abs() < 1e-5)
        );
    }
}
