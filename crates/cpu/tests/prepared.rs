use synarith_core::*;
use synarith_cpu::*;

#[test]
fn prepared_execution_matches_recursive_reference_through_mutation_and_resampling() {
    let fern = include_str!("../../../examples/barnsley-fern.syar")
        .replace("param points: int = 8192;", "param points: int = 256;");
    for source in [
        include_str!("../../../examples/clouds.syar"),
        include_str!("../../../examples/paths.syar"),
        include_str!("../../../examples/mandelbrot.syar"),
        include_str!("../../../examples/deep-space.syar"),
        &fern,
        "syar 1;use std;fn f(q:vec2)->float{var v=x(q);for i in 0..8{if i%2==0{continue;}v=sin(v+float(i));if v<0.0{break;}}while v<0.9 limit 4{v+=0.3;}return v;}let field=f(p);output main=blur(normal(sample_at(field,p*2.0),epsilon:0.01),0.01);",
        "syar 1;fn f(v:float)->float{var x=v;x=x*x+x;x=x+x*x;return x;}let v=f(x(p));output main=rgba(v+v,v*v,v);",
    ] {
        let p = compile(&parse(source).unwrap()).unwrap();
        let prepared = CpuProgram::compile(&p).unwrap();
        let parameters = p.defaults();
        for output in p.outputs.keys() {
            for (at, time, width) in [
                ([0.23, 0.41], 0.0, 0.0),
                ([0.64, 0.37], 0.7, 0.003),
                ([-0.4, 0.5], 1.2, 0.01),
            ] {
                let actual = prepared
                    .sample(&p, output, at, time, &parameters, Some(0xfedcba98), width)
                    .unwrap();
                let expected =
                    sample(&p, output, at, time, &parameters, Some(0xfedcba98), width).unwrap();
                assert_eq!(actual, expected, "{output} at {at:?}");
            }
        }
    }
}

#[test]
fn prepared_output_selection_and_parameters_are_checked() {
    let p = compile(
        &parse("syar 1;param v:float=0.2;output main=rgba(v,0,0);output other=rgba(0,v,0);")
            .unwrap(),
    )
    .unwrap();
    let prepared = CpuProgram::for_output(&p, "main").unwrap();
    assert!(
        prepared
            .sample(&p, "other", [0., 0.], 0., &p.defaults(), None, 0.)
            .is_err()
    );
    assert_eq!(
        prepared
            .sample(&p, "main", [0., 0.], 0., &[Value::float(0.7)], None, 0.)
            .unwrap()
            .data,
        [0.7, 0., 0., 1.]
    );
    assert!(
        prepared
            .sample(&p, "main", [0., 0.], 0., &[Value::int(1)], None, 0.)
            .is_err()
    );
    assert!(
        prepared
            .sample(&p, "main", [0., 0.], 0., &p.defaults(), None, -1.)
            .is_err()
    );
}

#[test]
fn shared_calls_use_registers_including_the_large_block_path() {
    let chain = format!("{}x(p){}", "sin(".repeat(40), ")".repeat(40));
    let source = format!(
        "syar 1;fn f(v:float)->float{{var total=v;for i in 0..64{{total=cos(total);}}return total;}}let v=f({chain});output main=rgba(v+v,v*v,v);"
    );
    let p = compile(&parse(&source).unwrap()).unwrap();
    let prepared = CpuProgram::compile(&p).unwrap();
    let block = prepared
        .execution_plan()
        .block(p.output("main").unwrap())
        .unwrap();
    assert!(block.reuse_registers);
    assert!(block.nodes.len() > 32);
    assert_eq!(
        prepared
            .sample(&p, "main", [0.3, 0.7], 0., &[], None, 0.)
            .unwrap(),
        sample(&p, "main", [0.3, 0.7], 0., &[], None, 0.).unwrap()
    );
    let other =
        compile(&parse("syar 1;output main=rgba(x(p),0,0);output other=rgba(0,y(p),0);").unwrap())
            .unwrap();
    let prepared = CpuProgram::for_output(&other, "main").unwrap();
    assert!(
        prepared
            .render_region(
                &other,
                &RenderOptions {
                    output: "other".into(),
                    width: 1,
                    height: 1,
                    ..Default::default()
                },
                Region {
                    x: 0,
                    y: 0,
                    width: 1,
                    height: 1
                }
            )
            .is_err()
    );
}
