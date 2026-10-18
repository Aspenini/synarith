use synarith_core::*;
use synarith_cpu::*;

#[test]
fn every_registered_operation_executes_on_cpu() {
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
        let source = format!("syar 1;output main = {expression};");
        let program =
            compile(&parse(&source).unwrap()).unwrap_or_else(|e| panic!("{}: {e}", op.name));
        let options = RenderOptions {
            width: 4,
            height: 4,
            time: 0.13,
            ..Default::default()
        };
        let pixels = render(&program, &options).unwrap_or_else(|e| panic!("{}: {e}", op.name));
        assert_eq!(pixels.len(), 16, "{}", op.name);
        assert!(
            pixels.iter().flatten().all(|v| v.is_finite()),
            "{}",
            op.name
        );
    }
}

#[test]
fn bundled_documents_render_without_graphics_dependencies() {
    // A shorter orbit keeps CPU coverage quick; the same function and all four maps run.
    let fern = include_str!("../../../examples/barnsley-fern.syar")
        .replace("param points: int = 8192;", "param points: int = 256;");
    let koch = include_str!("../../../examples/koch-snowflake.syar")
        .replace("param depth: int = 4;", "param depth: int = 2;");
    let dragon = include_str!("../../../examples/dragon-curve.syar")
        .replace("param depth: int = 10;", "param depth: int = 6;");
    for source in [
        koch.as_str(),
        dragon.as_str(),
        include_str!("../../../examples/clouds.syar"),
        include_str!("../../../examples/orbit.syar"),
        include_str!("../../../examples/tiles.syar"),
        include_str!("../../../examples/paths.syar"),
        include_str!("../../../examples/mandelbrot.syar"),
        include_str!("../../../examples/algorithms.syar"),
        include_str!("../../../examples/deep-space.syar"),
        fern.as_str(),
    ] {
        let program = compile(&parse(source).unwrap()).unwrap();
        let options = RenderOptions {
            width: 16,
            height: 12,
            time: 0.7,
            samples: 2,
            ..Default::default()
        };
        let pixels = render(&program, &options).unwrap();
        assert_eq!(pixels.len(), 192);
        assert!(pixels.iter().any(|p| p[3] > 0.));
    }
}
