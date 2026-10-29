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

#[test]
fn tile_documents_and_material_planes_match_cpu() {
    let Some(gpu) = adapter() else {
        return;
    };
    let tile = r##"syar 1;use std;tile;
let h = tile_fbm(p, 5.0, octaves: 4, seed: 2) + tile_cellular(p, 7.0, seed: 4) * 0.3;
output main = mix(srgb("#203040"), srgb("#e0d0a0"), h);
output bump = normal(h, strength: 0.05, epsilon: 0.02);
output mask = h;
material { albedo = mix(srgb("#203040"), srgb("#e0d0a0"), h); height = h * 0.01; roughness = h; }
"##;
    // Zoomed out, a tile document repeats instead of padding.
    for viewport in [None, Some([-1.0, -0.5, 3.0, 3.0])] {
        for output in ["main", "bump", "mask"] {
            compare(
                &gpu,
                tile,
                &format!("tile {output} {viewport:?}"),
                &RenderOptions {
                    width: 24,
                    height: 20,
                    viewport,
                    output: output.into(),
                    ..Default::default()
                },
            );
        }
    }
    let program = compile(&parse(tile).unwrap()).unwrap();
    let compiled = gpu.compile_material(&program).unwrap();
    let options = RenderOptions {
        width: 24,
        height: 24,
        samples: 2,
        ..Default::default()
    };
    let region = Region {
        x: 3,
        y: 0,
        width: 17,
        height: 11,
    };
    let actual = gpu
        .render_material_planes(&program, &compiled, &options, region)
        .unwrap();
    let expected = CpuProgram::for_material(&program)
        .unwrap()
        .render_material_planes(&program, &options, region)
        .unwrap();
    assert_eq!(actual.len(), expected.len());
    let difference = actual
        .iter()
        .flatten()
        .zip(expected.iter().flatten())
        .map(|(a, b)| (a - b).abs())
        .fold(0f32, f32::max);
    assert!(difference <= 0.004, "material planes: {difference}");
    assert!(
        gpu.render_region(&program, &compiled, &options, region)
            .is_err()
    );
}

// Run a host plan on a device the test owns, exactly as an application would,
// handing each strip's region and pixels to `take`.
fn run_host_plan(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    plan: &synarith_gpu::HostRender,
    mut take: impl FnMut(Region, Vec<Pixel>),
) {
    use wgpu::util::DeviceExt;
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(plan.shader.clone().into()),
    });
    // An explicit layout: a shader need not use every binding.
    let entry = |binding, ty| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    };
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[
            entry(0, wgpu::BufferBindingType::Uniform),
            entry(1, wgpu::BufferBindingType::Storage { read_only: true }),
            entry(2, wgpu::BufferBindingType::Storage { read_only: false }),
        ],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[&layout],
        push_constant_ranges: &[],
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&pipeline_layout),
        module: &module,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: &plan.parameters,
        usage: wgpu::BufferUsages::STORAGE,
    });
    let target = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: plan.buffer_bytes,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: plan.buffer_bytes,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    for strip in &plan.strips {
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: &strip.uniform,
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: params.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: target.as_entire_binding(),
                },
            ],
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(strip.workgroups[0], strip.workgroups[1], 1);
        }
        encoder.copy_buffer_to_buffer(&target, 0, &staging, 0, strip.bytes);
        queue.submit(Some(encoder.finish()));
        let slice = staging.slice(0..strip.bytes);
        slice.map_async(wgpu::MapMode::Read, |r| r.unwrap());
        device.poll(wgpu::Maintain::Wait);
        let pixels = synarith_gpu::decode_pixels(&slice.get_mapped_range());
        staging.unmap();
        take(strip.region, pixels);
    }
}

#[test]
fn host_plans_run_on_a_foreign_device() {
    use synarith_gpu::{HostLimits, host_material, host_output};
    if adapter().is_none() {
        return;
    }
    let (device, queue) = pollster::block_on(async {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
            .unwrap();
        adapter
            .request_device(&wgpu::DeviceDescriptor::default(), None)
            .await
            .unwrap()
    });
    let source = r##"syar 1;use std;tile;
let h = tile_fbm(p, 5.0, octaves: 4, seed: 2);
material { albedo = mix(srgb("#203040"), srgb("#e0d0a0"), h); height = h * 0.01; roughness = h; occlusion = 1.0 - h * 0.5; }
"##;
    let program = compile(&parse(source).unwrap()).unwrap();
    let options = RenderOptions {
        width: 24,
        height: 20,
        ..Default::default()
    };
    let limits = HostLimits {
        max_strip_values: 24 * 9,
        ..Default::default()
    };
    // Material strips written in place match a CPU render.
    let plan = host_material(&program, &options, limits).unwrap();
    assert!(plan.strips.len() > 1);
    let mut image = MaterialPixels::new(program.material.as_ref().unwrap(), 24, 20);
    run_host_plan(&device, &queue, &plan, |region, pixels| {
        image
            .write_region(&program, &options, region, &pixels)
            .unwrap();
    });
    let expected = render_material(&program, &options, options.full_region()).unwrap();
    for ((channel, a), (_, b)) in image.channels.iter().zip(&expected.channels) {
        let difference = a
            .to_pixels()
            .iter()
            .flatten()
            .zip(b.to_pixels().iter().flatten())
            .map(|(a, b)| (a - b).abs())
            .fold(0f32, f32::max);
        assert!(difference <= 0.004, "{channel}: {difference}");
    }
    // A single output's strips append in row order.
    let output = RenderOptions {
        output: "material.albedo".into(),
        ..options.clone()
    };
    let plan = host_output(&program, &output, limits).unwrap();
    let mut pixels = Vec::new();
    run_host_plan(&device, &queue, &plan, |_, part| pixels.extend(part));
    let expected = render(&program, &output).unwrap();
    assert_eq!(pixels.len(), expected.len());
    assert!(
        pixels
            .iter()
            .flatten()
            .zip(expected.iter().flatten())
            .all(|(a, b)| (a - b).abs() <= 0.004)
    );
}
