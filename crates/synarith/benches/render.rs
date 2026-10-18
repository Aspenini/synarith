//! Preparation, paired serial point evaluation, and warm parallel CPU rendering.
//! Serial cases use the same 8x8 points with both execution paths, avoiding thread
//! count and scheduling differences in the reference/prepared comparison.
use std::time::Instant;
use synarith::*;

fn main() {
    println!("case,phase,width,height,median_ms");
    for (name, source) in [
        ("noise", include_str!("../../../examples/clouds.syar")),
        ("paths", include_str!("../../../examples/paths.syar")),
        (
            "warps",
            "syar 1;use std;let field=fbm(p*10.0);let flow=vec2(noise(p*4.0),noise(p*4.0,seed:17))*0.15;output main=mix(srgb(\"#101831\"),srgb(\"#55ebcc\"),warp(field,flow));",
        ),
        (
            "shared_iteration",
            "syar 1;fn field(q:vec2)->float{var v=x(q);for i in 0..128{v=sin(v+float(i)*0.01);}return v*0.5+0.5;}let v=field(p);output main=rgba((v+v+v+v+v+v+v+v+v+v+v+v+v+v+v+v)/16.0,v*v,v);",
        ),
    ] {
        let program = load(source).unwrap();
        let parameters = program.defaults();
        let start = Instant::now();
        let prepared = CpuProgram::for_output(&program, "main").unwrap();
        println!(
            "{name},prepare,0,0,{:.3}",
            start.elapsed().as_secs_f64() * 1000.0
        );
        // Same points, parameters, and serial work for a fair evaluator comparison.
        // The recursive path remains the semantic reference implementation.
        for scheduled in [false, true] {
            let mut times = vec![];
            for _ in 0..3 {
                let start = Instant::now();
                for y in 0..8 {
                    for x in 0..8 {
                        let p = [(x as f32 + 0.5) / 8.0, (y as f32 + 0.5) / 8.0];
                        let value = if scheduled {
                            prepared.sample(
                                &program,
                                "main",
                                p,
                                0.7,
                                &parameters,
                                None,
                                1.0 / 128.0,
                            )
                        } else {
                            sample(&program, "main", p, 0.7, &parameters, None, 1.0 / 128.0)
                        }
                        .unwrap();
                        std::hint::black_box(value);
                    }
                }
                times.push(start.elapsed().as_secs_f64() * 1000.0);
            }
            times.sort_by(f64::total_cmp);
            println!(
                "{name},{},8,8,{:.3}",
                if scheduled {
                    "prepared_points"
                } else {
                    "reference_points"
                },
                times[1]
            );
        }
        let mut renderer = Renderer::new(program);
        let options = RenderOptions {
            width: 128,
            height: 128,
            time: 0.7,
            ..Default::default()
        };
        renderer.render(&options).unwrap();
        let mut times = vec![];
        for _ in 0..3 {
            let start = Instant::now();
            std::hint::black_box(renderer.render(&options).unwrap());
            times.push(start.elapsed().as_secs_f64() * 1000.);
        }
        times.sort_by(f64::total_cmp);
        println!("{name},parallel_render,128,128,{:.3}", times[1]);
        // Same image size and loop work at each scale; buffers and program size
        // stay constant. Report actual timings rather than promising a frame rate.
        if name == "shared_iteration" {
            for zoom in [1.0, 1e6, 1e12] {
                let deep = RenderOptions {
                    precise_viewport: Some([
                        0.5 - 0.5 / zoom,
                        0.5 - 0.5 / zoom,
                        1. / zoom,
                        1. / zoom,
                    ]),
                    ..options.clone()
                };
                renderer.render(&deep).unwrap();
                let mut times = vec![];
                for _ in 0..3 {
                    let start = Instant::now();
                    std::hint::black_box(renderer.render(&deep).unwrap());
                    times.push(start.elapsed().as_secs_f64() * 1000.);
                }
                times.sort_by(f64::total_cmp);
                println!("{name},f64_zoom_{zoom:.0e},128,128,{:.3}", times[1]);
            }
        }
    }
}
