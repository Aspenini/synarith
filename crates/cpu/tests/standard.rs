use synarith_core::*;
use synarith_cpu::*;

#[test]
fn source_blends_preserve_premultiplied_alpha() {
    for (name, expected) in [
        ("multiply", [0.25, 0.0, 0.25, 0.75]),
        ("screen", [0.5, 0.0, 0.5, 0.75]),
        ("overlay", [0.25, 0.0, 0.5, 0.75]),
    ] {
        let program = compile(
            &parse(&format!(
                "syar 1;use std;output main={name}(rgba(1,0,0,0.5),rgba(0,0,1,0.5));"
            ))
            .unwrap(),
        )
        .unwrap();
        let actual = sample(&program, "main", [0., 0.], 0., &[], None, 0.)
            .unwrap()
            .data;
        assert!(
            actual
                .iter()
                .zip(expected)
                .all(|(a, b)| (a - b).abs() < 1e-6),
            "{name}: {actual:?}"
        );
    }
}

#[test]
fn sampling_primitives_track_coordinates_and_zero_footprints() {
    let program = compile(
        &parse("syar 1;output main=rgba(footprint(x(p)*2.0),footprint(p*3.0),pixel_size());")
            .unwrap(),
    )
    .unwrap();
    let actual = sample(&program, "main", [0.5, 0.5], 0., &[], None, 0.01)
        .unwrap()
        .data;
    assert!((actual[0] - 0.02).abs() < 1e-6);
    assert!((actual[1] - 0.03).abs() < 1e-6);
    assert_eq!(actual[2], 0.01);
    assert_eq!(
        sample(&program, "main", [0.5, 0.5], 0., &[], None, 0.)
            .unwrap()
            .data,
        [0., 0., 0., 1.]
    );
}

#[test]
fn every_public_source_library_algorithm_renders() {
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
        let source = format!("syar 1;use std;seed 4294967295;output main={output};");
        let doc = parse(&source).unwrap();
        let program = compile(&doc).unwrap_or_else(|e| panic!("{name}: {e}"));
        let options = RenderOptions {
            width: 4,
            height: 4,
            samples: 2,
            ..Default::default()
        };
        let actual = render(&program, &options).unwrap();
        assert!(actual.iter().flatten().all(|v| v.is_finite()), "{name}");
        let standalone = compile(&parse(&expand_standard_library(&doc).unwrap()).unwrap()).unwrap();
        assert_eq!(actual, render(&standalone, &options).unwrap(), "{name}");
    }
}
