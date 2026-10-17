use std::fmt::Write;
use synarith_core::*;

pub fn generate(program: &Program, output: &str) -> Result<String, String> {
    let root = program.output(output)?;
    let mut source = include_str!("common.wgsl").to_owned();
    for (id, node) in program.nodes.iter().enumerate() {
        writeln!(source, "fn n{id}(p:vec2<f32>,fp:f32)->vec4<f32>{{").unwrap();
        for (i, input) in node.inputs.iter().enumerate() {
            // Sampling sources must be called only at their requested positions.
            if i == 0
                && matches!(
                    node.kind,
                    NodeKind::Builtin(Op::Warp | Op::Transform | Op::Blur | Op::Normal)
                )
            {
                continue;
            }
            writeln!(source, "let {}=n{input}(p,fp);", (b'a' + i as u8) as char).unwrap();
        }
        let expression = match &node.kind {
            NodeKind::Constant(v) => format!(
                "vec4<f32>({:.9e},{:.9e},{:.9e},{:.9e})",
                v.data[0], v.data[1], v.data[2], v.data[3]
            ),
            NodeKind::Position => "vec4<f32>(p,0.0,0.0)".into(),
            NodeKind::Time => "vec4<f32>(cfg.clock.x)".into(),
            NodeKind::Parameter(index) => format!("params[{index}]"),
            NodeKind::Unary(op) => match op.as_str() {
                "-" => "-a",
                "!" => "vec4<f32>(select(0.0,1.0,a.x==0.0))",
                _ => "a",
            }
            .into(),
            NodeKind::Binary(op) => binary(op),
            NodeKind::Builtin(op) => builtin(*op, node, &mut source),
        };
        writeln!(source, "return clean({expression});\n}}").unwrap();
    }
    write!(source, r#"
@compute @workgroup_size(8,8)
fn main(@builtin(global_invocation_id) gid:vec3<u32>) {{
    if gid.x>=cfg.state.x || gid.y>=cfg.state.y {{return;}}
    var sum=vec4<f32>(0.0);let samples=cfg.state.z;
    for(var sy=0u;sy<samples;sy++) {{for(var sx=0u;sx<samples;sx++) {{
        let pixel=vec2<f32>(cfg.size.zw+gid.xy)+(vec2<f32>(f32(sx),f32(sy))+0.5)/f32(samples);
        let p=cfg.geom.xy+pixel*cfg.geom.z;
        if all(p>=cfg.viewbox.xy) && all(p<=cfg.viewbox.xy+cfg.viewbox.zw) {{sum+=n{root}(p,cfg.geom.z/f32(samples));}}
    }}}}
    pixels[gid.y*cfg.state.x+gid.x]=clean(sum/f32(samples*samples));
}}
"#).unwrap();
    Ok(source)
}
fn binary(op: &str) -> String {
    match op {
        "+" | "-" | "*" => format!("a{op}b"),
        "/" => "vd(a,b)".into(),
        "%" => "a-b*floor(vd(a,b))".into(),
        "&&" => "vec4<f32>(select(0.0,1.0,a.x!=0.0 && b.x!=0.0))".into(),
        "||" => "vec4<f32>(select(0.0,1.0,a.x!=0.0 || b.x!=0.0))".into(),
        _ => format!("vec4<f32>(select(0.0,1.0,a.x{op}b.x))"),
    }
}
fn footprint(node: &Node, source: &mut String) {
    let input = node.inputs[0];
    writeln!(source,"let center=n{input}(p,0.0).xy;let dx=n{input}(p+vec2<f32>(fp,0.0),0.0).xy;let dy=n{input}(p+vec2<f32>(0.0,fp),0.0).xy;let footprint=select(0.0,max(length(dx-center),length(dy-center)),fp>0.0);").unwrap();
}
fn builtin(op: Op, node: &Node, source: &mut String) -> String {
    use Op::*;
    match op {
        Mandelbrot => "vec4<f32>(mandelbrot(a.xy,clamp(i32(b.x),1,1024)))".into(),
        Add=>"a+b".into(),Sub=>"a-b".into(),Mul=>"a*b".into(),Div=>"vd(a,b)".into(),Mod=>"a-b*floor(vd(a,b))".into(),
        Less=>binary("<"),Greater=>binary(">"),Equal=>binary("=="),Not=>"vec4<f32>(select(0.0,1.0,a.x==0.0))".into(),And=>binary("&&"),Or=>binary("||"),
        ToInt=>"vec4<f32>(trunc(clamp(a.x,-16777216.0,16777216.0)))".into(),ToFloat|X=>"vec4<f32>(a.x)".into(),Y=>"vec4<f32>(a.y)".into(),
        Vec2=>"vec4<f32>(a.x,b.x,0.0,0.0)".into(),Rgba=>"vec4<f32>(vec3<f32>(a.x,b.x,c.x)*clamp(d.x,0.0,1.0),clamp(d.x,0.0,1.0))".into(),
        Red|Green|Blue=>format!("vec4<f32>(sd(a.{},a.w))",match op {Red=>"x",Green=>"y",_=>"z"}),Alpha=>"vec4<f32>(a.w)".into(),
        Sin=>"sin(a)".into(),Cos=>"cos(a)".into(),Tan=>"tan(a)".into(),Abs=>"abs(a)".into(),Floor=>"floor(a)".into(),Ceil=>"ceil(a)".into(),Fract=>"a-floor(a)".into(),
        Sqrt=>"sqrt(max(a,vec4<f32>(0.0)))".into(),Exp=>"exp(min(a,vec4<f32>(80.0)))".into(),
        Log=>"select(vec4<f32>(0.0),log(max(a,vec4<f32>(1e-38))),a>vec4<f32>(0.0))".into(),
        Pow=>"vec4<f32>(power(a.x,b.x),power(a.y,b.x),power(a.z,b.x),power(a.w,b.x))".into(),
        Min=>"min(a,b)".into(),Max=>"max(a,b)".into(),Clamp=>"clamp(a,vec4<f32>(min(b.x,c.x)),vec4<f32>(max(b.x,c.x)))".into(),
        Step=>"vec4<f32>(select(0.0,1.0,b.x>=a.x))".into(),Smoothstep=>"vec4<f32>(sm(a.x,b.x,c.x))".into(),Mix=>"a+(b-a)*c.x".into(),Select=>"select(c,b,a.x!=0.0)".into(),
        Length=>"vec4<f32>(length(a.xy))".into(),Dot=>"vec4<f32>(dot(a.xy,b.xy))".into(),Normalize=>"vec4<f32>(sd(a.x,length(a.xy)),sd(a.y,length(a.xy)),0.0,0.0)".into(),
        Noise|GradientNoise=>{
            footprint(node,source);
            source.push_str("let band=1.0-sm(0.25,0.75,footprint);\n");
            format!("vec4<f32>(noise(a.xy,cfg.state.w ^ bitcast<u32>(i32(b.x)),vec2<i32>(0),{})*band+0.5*(1.0-band))",op==GradientNoise)
        },
        Cellular=>"vec4<f32>(cellular(a.xy,cfg.state.w ^ bitcast<u32>(i32(b.x))))".into(),
        PeriodicNoise=>"vec4<f32>(noise(a.xy,cfg.state.w ^ bitcast<u32>(i32(c.x)),vec2<i32>(max(floor(b.xy+0.5),vec2<f32>(1.0))),false))".into(),
        Fbm|Turbulence|Ridged=>{
            footprint(node,source);
            source.push_str("var freq=1.0;var amp=1.0;var sum=0.0;var weight=0.0;let seed=cfg.state.w ^ bitcast<u32>(i32(e.x));\nfor(var octave=0;octave<clamp(i32(b.x),1,12);octave++){let band=1.0-sm(0.25,0.75,footprint*freq);let raw=noise(a.xy*freq,seed+u32(octave),vec2<i32>(0),false);\n");
            source.push_str(match op {Turbulence=>"let value=abs(raw*2.0-1.0);",Ridged=>"let value=1.0-abs(raw*2.0-1.0);",_=>"let value=raw;"});
            source.push_str("sum+=(value*band+0.5*(1.0-band))*amp;weight+=amp;freq*=clamp(abs(c.x),0.01,16.0);amp*=clamp(d.x,0.0,1.0);}\n");
            "vec4<f32>(sd(sum,weight))".into()
        },
        Linear=>"vec4<f32>(sd(dot(a.xy-b.xy,c.xy-b.xy),dot(c.xy-b.xy,c.xy-b.xy)))".into(),Radial=>"vec4<f32>(length(a.xy-b.xy)/max(abs(c.x),1e-8))".into(),Angular=>"vec4<f32>(atan2(a.y-b.y,a.x-b.x)/TAU+0.5)".into(),
        Ramp=>{
            source.push_str("let p1=clamp(f.x,0.0001,0.9998);let p2=clamp(g.x,p1+0.0001,0.9999);let x=clamp(a.x,0.0,1.0);var result=vec4<f32>(0.0);if x<p1 {result=b+(c-b)*(x/p1);}else if x<p2 {result=c+(d-c)*((x-p1)/(p2-p1));}else{result=d+(e-d)*((x-p2)/(1.0-p2));}\n");"result".into()
        },
        Circle=>"vec4<f32>(length(a.xy)-abs(b.x))".into(),
        Box|RoundedBox=>{
            writeln!(source,"let r={};let q=abs(a.xy)-abs(b.xy)+r;",if op==RoundedBox {"min(abs(c.x),min(abs(b.x),abs(b.y)))"}else{"0.0"}).unwrap();
            "vec4<f32>(length(max(q,vec2<f32>(0.0)))+min(max(q.x,q.y),0.0)-r)".into()
        },
        Ellipse=>{source.push_str("let radii=max(abs(b.xy),vec2<f32>(1e-8));\n");"vec4<f32>((length(a.xy/radii)-1.0)*min(radii.x,radii.y))".into()},
        Polygon=>"vec4<f32>(polygon(a.xy,clamp(i32(b.x),3,64),abs(c.x),abs(c.x)))".into(),Star=>"vec4<f32>(polygon(a.xy,clamp(i32(b.x),2,32)*2,abs(c.x),abs(d.x)))".into(),
        Bezier=>"vec4<f32>(path_distance(a.xy,b.xy,c.xy,d.xy,e.xy,64,false)-abs(f.x)*0.5)".into(),
        Path=>{source.push_str("let curvature=6.0*max(length(b.xy-2.0*c.xy+d.xy),length(c.xy-2.0*d.xy+e.xy));let count=i32(clamp(ceil(sqrt(curvature/(8.0*max(abs(g.x),1e-8)))),8.0,256.0));\n");"vec4<f32>(path_distance(a.xy,b.xy,c.xy,d.xy,e.xy,count,f.x!=0.0))".into()},
        Union=>"vec4<f32>(min(a.x,b.x))".into(),Intersect=>"vec4<f32>(max(a.x,b.x))".into(),Subtract=>"vec4<f32>(max(a.x,-b.x))".into(),
        SmoothUnion=>{source.push_str("let r=max(abs(c.x),1e-8);let h=clamp(0.5+0.5*(b.x-a.x)/r,0.0,1.0);\n");"vec4<f32>(b.x+(a.x-b.x)*h-r*h*(1.0-h))".into()},
        Fill|Stroke|Outline=>{
            let input=node.inputs[0];
            writeln!(source,"var width=1e-7;if fp>0.0 {{let dx=n{input}(p+vec2<f32>(fp*0.5,0.0),0.0).x-n{input}(p-vec2<f32>(fp*0.5,0.0),0.0).x;let dy=n{input}(p+vec2<f32>(0.0,fp*0.5),0.0).x-n{input}(p-vec2<f32>(0.0,fp*0.5),0.0).x;width=max(abs(dx)+abs(dy),fp*0.05);}}").unwrap();
            match op {Fill=>"b*(1.0-sm(-width*0.5,width*0.5,a.x))",Stroke=>"c*(1.0-sm(-width*0.5,width*0.5,abs(a.x)-abs(b.x)*0.5))",_=>"c*(1.0-sm(-width*0.5,width*0.5,abs(a.x-abs(b.x)*0.5)-abs(b.x)*0.5))"}.into()
        },
        Glow=>"c*exp(-max(a.x,0.0)/max(abs(b.x),1e-8))".into(),Translate=>"vec4<f32>(a.xy-b.xy,0.0,0.0)".into(),Rotate=>"vec4<f32>(cos(b.x)*a.x-sin(b.x)*a.y,sin(b.x)*a.x+cos(b.x)*a.y,0.0,0.0)".into(),Scale=>"vec4<f32>(a.xy*b.xy,0.0,0.0)".into(),
        Repeat=>{source.push_str("let period=max(abs(b.xy),vec2<f32>(1e-8));\n");"vec4<f32>(a.xy-period*floor((a.xy+period*0.5)/period),0.0,0.0)".into()},Mirror=>"vec4<f32>(abs(a.xy),0.0,0.0)".into(),Polar=>"vec4<f32>(length(a.xy),atan2(a.y,a.x),0.0,0.0)".into(),
        Warp|Transform=>{
            let input=node.inputs[1];let src=node.inputs[0];let identity=if op==Warp {"fp"}else{"0.0"};
            writeln!(source,"var transformed_fp=0.0;if fp>0.0 {{let center=n{input}(p,0.0).xy;let dx=n{input}(p+vec2<f32>(fp,0.0),0.0).xy;let dy=n{input}(p+vec2<f32>(0.0,fp),0.0).xy;transformed_fp=max(length(dx-center+vec2<f32>({identity},0.0)),length(dy-center+vec2<f32>(0.0,{identity})));}}").unwrap();
            format!("n{src}({},transformed_fp)",if op==Warp {"p+b.xy"}else{"b.xy"})
        },
        Over|Multiply|Screen|Overlay=>format!("blend(a,b,{})",match op {Multiply=>1,Screen=>2,Overlay=>3,_=>0}),
        Mask|Opacity=>"a*clamp(b.x,0.0,1.0)".into(),Invert=>"vec4<f32>(vec3<f32>(a.w)-a.xyz,a.w)".into(),Grayscale=>"vec4<f32>(vec3<f32>(dot(a.xyz,vec3<f32>(0.2126,0.7152,0.0722))),a.w)".into(),Contrast=>"vec4<f32>((a.xyz-a.w*0.5)*b.x+a.w*0.5,a.w)".into(),
        Saturate=>{source.push_str("let l=vec3<f32>(dot(a.xyz,vec3<f32>(0.2126,0.7152,0.0722)));\n");"vec4<f32>(l+(a.xyz-l)*b.x,a.w)".into()},
        Loop=>"vec4<f32>(fract(sd(a.x,max(abs(b.x),1e-8))))".into(),Pulse=>"vec4<f32>(0.5-0.5*cos(a.x*TAU))".into(),
        Blur=>{let src=node.inputs[0];writeln!(source,"var sum=vec4<f32>(0.0);let radius=max(b.x,0.0);for(var y=-1;y<=1;y++){{for(var x=-1;x<=1;x++){{let weight=select(1.0,2.0,x==0)*select(1.0,2.0,y==0);sum+=n{src}(p+vec2<f32>(f32(x),f32(y))*radius,fp)*weight/16.0;}}}}").unwrap();"sum".into()},
        Normal=>{let src=node.inputs[0];writeln!(source,"let epsilon=max(abs(c.x),1e-6);let dx=(n{src}(p+vec2<f32>(epsilon,0.0),fp).x-n{src}(p-vec2<f32>(epsilon,0.0),fp).x)/(2.0*epsilon);let dy=(n{src}(p+vec2<f32>(0.0,epsilon),fp).x-n{src}(p-vec2<f32>(0.0,epsilon),fp).x)/(2.0*epsilon);let normal=normalize(vec3<f32>(-dx*b.x,-dy*b.x,1.0));").unwrap();"vec4<f32>(normal*0.5+0.5,1.0)".into()},
    }
}
