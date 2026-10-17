use std::time::Instant;
use synarith::*;

fn main() {
    println!("case,width,height,median_ms");
    for (name, source) in [
        ("noise", include_str!("../../../examples/clouds.syar")),
        ("paths", include_str!("../../../examples/paths.syar")),
        (
            "warps",
            "syar 1;let field=fbm(p*10.0);let flow=vec2(noise(p*4.0),noise(p*4.0,seed:17))*0.15;output main=mix(srgb(\"#101831\"),srgb(\"#55ebcc\"),warp(field,flow));",
        ),
    ] {
        let mut renderer = Renderer::new(load(source).unwrap());
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
        println!("{name},128,128,{:.3}", times[1]);
    }
}
