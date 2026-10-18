use synarith_core::*;
use synarith_cpu::*;

fn evaluate(source: &str) -> Pixel {
    let program = compile(&parse(source).unwrap()).unwrap();
    sample(
        &program,
        "main",
        [0.3, 0.6],
        0.4,
        &program.defaults(),
        Some(19),
        0.002,
    )
    .unwrap()
    .data
}
#[test]
fn structured_control_flow_and_isolated_call_frames() {
    let source = r#"syar 1;use std;
fn square(v:float)->float {return v*v;}
fn count(n:int)->float {
    var total:float=0.0;
    for i in 0..8 {
        if i>=n {break;}
        if i%2==0 {continue;}
        total+=square(float(i));
    }
    return total;
}
fn bounded()->float {
    var i=0;
    var sum=0;
    while i<20 limit 4 {
        i+=1;
        if i==2 {continue;}
        sum+=i;
    }
    return float(sum);
}
fn nested()->float {
    var result=0;
    for a in 1..4 {
        for b in 0..4 {
            if b==2 {break;}
            result+=a;
        }
    }
    return float(result);
}
output main=rgba(count(6)+count(n:4),bounded(),nested());
"#;
    assert_eq!(evaluate(source), [45., 8., 12., 1.]);
    assert_eq!(
        evaluate(
            "syar 1;use std;fn f()->float{if t>0 {return 0.3;} else {return 0.8;}}output main=rgba(f(),0,0);"
        ),
        [0.3, 0., 0., 1.]
    );
    assert_eq!(
        evaluate(
            "syar 1;use std;fn f()->float{var v=2;for i in 0..0{v+=1;}while true limit 0{v+=1;}return float(v);}output main=rgba(f(),0,0);"
        ),
        [2., 0., 0., 1.]
    );
}

#[test]
fn iterative_fields_resample_with_correct_function_context() {
    let actual = evaluate(
        "syar 1;use std;fn field(q:vec2)->float{var result=0.0;for i in 0..3{let offset=float(i)*0.2;result+=noise(q+vec2(offset,0.0),seed:7);}return result/3.0;}output main=blur(normal(sample_at(field(p),p*2.0),epsilon:0.01),0.01);",
    );
    let expected = evaluate(
        "syar 1;use std;let field=(noise(p,seed:7)+noise(p+vec2(0.2,0),seed:7)+noise(p+vec2(0.4,0),seed:7))/3.0;output main=blur(normal(sample_at(field,p*2.0),epsilon:0.01),0.01);",
    );
    for (a, b) in actual.into_iter().zip(expected) {
        assert!((a - b).abs() < 1e-5, "{actual:?} vs {expected:?}");
    }
}

#[test]
fn newton_iteration_and_space_wallpaper_are_repeatable_and_periodic() {
    let value = evaluate(
        "syar 1;use std;fn root(a:float)->float{var r=1.0;var error=1.0;while error>0.00001 limit 32{let next=0.5*(r+a/r);error=abs(next-r);r=next;}return r;}output main=rgba(root(2.0),root(9.0),0);",
    );
    assert!((value[0] - 2f32.sqrt()).abs() < 1e-5);
    assert!((value[1] - 3.).abs() < 1e-5);
    let program =
        compile(&parse(include_str!("../../../examples/deep-space.syar")).unwrap()).unwrap();
    let options = RenderOptions {
        width: 16,
        height: 9,
        ..Default::default()
    };
    let first = render(&program, &options).unwrap();
    assert_eq!(first, render(&program, &options).unwrap());
    assert_eq!(
        first,
        render(
            &program,
            &RenderOptions {
                time: 24.,
                ..options.clone()
            }
        )
        .unwrap()
    );
    assert_ne!(
        first,
        render(
            &program,
            &RenderOptions {
                time: 6.,
                ..options.clone()
            }
        )
        .unwrap()
    );
    let region = Region {
        x: 3,
        y: 2,
        width: 7,
        height: 4,
    };
    let tile = render_region(&program, &options, region).unwrap();
    for y in 0..region.height {
        for x in 0..region.width {
            assert_eq!(
                tile[(y * region.width + x) as usize],
                first[((y + region.y) * options.width + x + region.x) as usize]
            );
        }
    }
}
