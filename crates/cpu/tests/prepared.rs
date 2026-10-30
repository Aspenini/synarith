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

// Batched rendering evaluates every node across many samples at once; it must
// produce exactly the pixels of the per-sample interpreter, sample by sample.
#[test]
fn batched_renders_equal_the_per_sample_interpreter() {
    let fern = include_str!("../../../examples/barnsley-fern.syar")
        .replace("param points: int = 8192;", "param points: int = 256;");
    let library: String = {
        // Every source-library algorithm, through a branching, looping function.
        "syar 1;use std;tile;\n".to_string()
            + "fn f(q:vec2)->float{var v=x(q);for i in 0..8{if i%2==0{continue;}v=sin(v+float(i));if v<0.0{break;}}while v<0.9 limit 4{v+=0.3;}return v;}\n"
            + "let h=tile_fbm(p,5.0,octaves:4,seed:2)+tile_cellular(p,7.0)*0.3+tile_ridged(p,3.0)*0.2;\n"
            + "output a=blur(normal(sample_at(f(p),p*2.0),epsilon:0.01),0.01);\n"
            + "output b=fill(star(p-vec2(0.5,0.5),points:5),rgba(0.9,0.4,0.1));\n"
            + "output c=over(stroke(path(p),0.02,rgba(1,1,1,0.5)),overlay(rgba(h,0.2,0.4),glow(circle(p-0.5,0.2),0.1)));\n"
            + "output d=h;\n"
            + "material{albedo=rgba(h,h*0.5,0.2);height=h*0.01;roughness=h;occlusion=coverage(box(p-0.5,vec2(0.3,0.2)));}\n"
    };
    for source in [
        include_str!("../../../examples/clouds.syar"),
        include_str!("../../../examples/paths.syar"),
        include_str!("../../../examples/mandelbrot.syar"),
        include_str!("../../../examples/tiles.syar"),
        include_str!("../../../examples/brick-material.syar"),
        &fern,
        &library,
    ] {
        let p = compile(&parse(source).unwrap()).unwrap();
        let parameters = p.defaults();
        for output in p.outputs.keys() {
            for samples in [1, 2] {
                // A view wider than the document exercises padding and tile wrapping.
                let options = RenderOptions {
                    width: 9,
                    height: 7,
                    samples,
                    time: 0.7,
                    viewport: Some([-0.25, -0.1, 1.5, 1.2]),
                    output: output.clone(),
                    ..Default::default()
                };
                let pixels = render(&p, &options).unwrap();
                let mapping = Mapping::new(options.viewport.unwrap(), 9, 7);
                for y in 0..7 {
                    for x in 0..9 {
                        let mut total = [0f32; 4];
                        for sy in 0..samples {
                            for sx in 0..samples {
                                let s = samples as f32;
                                let at = mapping.position(
                                    x as f32 + (sx as f32 + 0.5) / s,
                                    y as f32 + (sy as f32 + 0.5) / s,
                                );
                                if !p.tile && !mapping.contains(at) {
                                    continue;
                                }
                                let evaluator = Evaluator {
                                    program: &p,
                                    parameters: &parameters,
                                    time: 0.7,
                                    seed: p.seed,
                                };
                                let id = p.output(output).unwrap();
                                let value = evaluator.eval(
                                    id,
                                    evaluator.wrap(at),
                                    mapping.units_per_pixel / s,
                                );
                                for i in 0..4 {
                                    total[i] += value[i];
                                }
                            }
                        }
                        let expected = total.map(|v| finite(v / (samples * samples) as f32));
                        assert_eq!(
                            pixels[y * 9 + x],
                            expected,
                            "{output} at {x},{y} with {samples}"
                        );
                    }
                }
            }
        }
        // Fused material planes equal the material outputs rendered one by one.
        if let Some(material) = &p.material {
            let options = RenderOptions {
                width: 16,
                height: 16,
                ..Default::default()
            };
            let fused = render_material(&p, &options, options.full_region()).unwrap();
            for channel in material.planes().concat() {
                let single = render(
                    &p,
                    &RenderOptions {
                        output: channel.output_name(),
                        ..options.clone()
                    },
                )
                .unwrap();
                assert_eq!(fused.get(channel).unwrap().to_pixels(), single, "{channel}");
            }
        }
    }
}
