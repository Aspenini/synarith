use std::fmt::Write;
use synarith_core::*;

pub fn generate(program: &Program, output: &str) -> Result<String, String> {
    let root = program.output(output)?;
    let plan = ExecutionPlan::for_output(program, output)?;
    let mut source = include_str!("common.wgsl").to_owned();
    let slots = plan
        .functions
        .iter()
        .map(|i| program.functions[*i].locals.len())
        .max()
        .unwrap_or(1)
        .max(1);
    writeln!(source, "alias Frame = array<vec4<f32>,{slots}>;").unwrap();
    for block in plan.blocks() {
        let id = block.root;
        writeln!(
            source,
            "fn n{id}(p:vec2<f32>,fp:f32,env:ptr<function,Frame>)->vec4<f32>{{"
        )
        .unwrap();
        for (register, instruction) in block.nodes.iter().enumerate() {
            let node = &program.nodes[instruction.node];
            writeln!(source, "var r{register}:vec4<f32>;{{").unwrap();
            for (i, input) in instruction.inputs.iter().enumerate() {
                if let Some(input) = input {
                    writeln!(source, "let {}=r{input};", (b'a' + i as u8) as char).unwrap();
                }
            }
            let expression = match &node.kind {
                NodeKind::Constant(v) => format!(
                    "vec4<f32>({:.9e},{:.9e},{:.9e},{:.9e})",
                    v.data[0], v.data[1], v.data[2], v.data[3]
                ),
                NodeKind::Local(slot) => format!("(*env)[{slot}]"),
                NodeKind::Call(index) => format!(
                    "f{index}(p,fp{})",
                    (0..node.inputs.len())
                        .map(|i| format!(",{}", (b'a' + i as u8) as char))
                        .collect::<String>()
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
                NodeKind::Builtin(op) => builtin(*op, node, program, &mut source),
            };
            writeln!(source, "r{register}=clean({expression});}}").unwrap();
        }
        writeln!(source, "return r{};}}", block.nodes.len() - 1).unwrap();
    }
    user_functions(program, &plan, &mut source);
    write!(source, r#"
@compute @workgroup_size(8,8)
fn main(@builtin(global_invocation_id) gid:vec3<u32>) {{
    if gid.x>=cfg.state.x || gid.y>=cfg.state.y {{return;}}
    var root_env:Frame;let env=&root_env;
    var sum=vec4<f32>(0.0);let samples=cfg.state.z;
    for(var sy=0u;sy<samples;sy++) {{for(var sx=0u;sx<samples;sx++) {{
        let pixel=vec2<f32>(cfg.size.zw+gid.xy)+(vec2<f32>(f32(sx),f32(sy))+0.5)/f32(samples);
        let p=cfg.geom.xy+pixel*cfg.geom.z;
        if all(p>=cfg.viewbox.xy) && all(p<=cfg.viewbox.xy+cfg.viewbox.zw) {{sum+=n{root}(p,cfg.geom.z/f32(samples),env);}}
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
fn user_functions(program: &Program, plan: &ExecutionPlan, source: &mut String) {
    for index in &plan.functions {
        let function = &program.functions[*index];
        write!(source, "fn f{index}(p:vec2<f32>,fp:f32").unwrap();
        for i in 0..function.parameters.len() {
            write!(source, ",arg{i}:vec4<f32>").unwrap();
        }
        source.push_str(")->vec4<f32>{\nvar local_env:Frame;let env=&local_env;\n");
        for i in 0..function.parameters.len() {
            writeln!(source, "local_env[{i}]=arg{i};").unwrap();
        }
        instructions(&function.body, source, &mut 0);
        source.push_str("}\n");
    }
}
fn instructions(body: &[Instruction], source: &mut String, counter: &mut usize) {
    for instruction in body {
        match instruction {
            Instruction::Store { slot, value } => {
                writeln!(source, "(*env)[{slot}]=n{value}(p,fp,env);").unwrap()
            }
            Instruction::Return(value) => writeln!(source, "return n{value}(p,fp,env);").unwrap(),
            Instruction::Break => source.push_str("break;\n"),
            Instruction::Continue => source.push_str("continue;\n"),
            Instruction::If { condition, yes, no } => {
                writeln!(source, "if n{condition}(p,fp,env).x!=0.0 {{").unwrap();
                instructions(yes, source, counter);
                source.push_str("}else{\n");
                instructions(no, source, counter);
                source.push_str("}\n");
            }
            Instruction::For {
                slot,
                start,
                end,
                body,
            } => {
                let id = *counter;
                *counter += 1;
                writeln!(source, "for(var i{id}={start}u;i{id}<{end}u;i{id}++){{\n(*env)[{slot}]=vec4<f32>(f32(i{id}));").unwrap();
                instructions(body, source, counter);
                source.push_str("}\n");
            }
            Instruction::While {
                condition,
                limit,
                body,
            } => {
                let id = *counter;
                *counter += 1;
                writeln!(source, "for(var i{id}=0u;i{id}<{limit}u;i{id}++){{\nif n{condition}(p,fp,env).x==0.0{{break;}}").unwrap();
                instructions(body, source, counter);
                source.push_str("}\n");
            }
        }
    }
}
fn footprint(node: &Node, source: &mut String) {
    let input = node.inputs[0];
    writeln!(source,"let center=n{input}(p,0.0,env).xy;let dx=n{input}(p+vec2<f32>(fp,0.0),0.0,env).xy;let dy=n{input}(p+vec2<f32>(0.0,fp),0.0,env).xy;let footprint=select(0.0,max(length(dx-center),length(dy-center)),fp>0.0);").unwrap();
}
fn builtin(op: Op, node: &Node, program: &Program, source: &mut String) -> String {
    use Op::*;
    match op {
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
        Atan2=>"vec4<f32>(atan2(a.x,b.x))".into(),
        Hash=>"vec4<f32>(hash2(vec2<i32>(clamp(floor(a.xy),vec2<f32>(-2147483648.0),vec2<f32>(2147483520.0))),(cfg.state.w ^ bitcast<u32>(i32(b.x)))+bitcast<u32>(i32(c.x)) ^ (bitcast<u32>(i32(d.x))*0x68bc21ebu)))".into(),
        PixelSize=>"vec4<f32>(fp)".into(),
        Footprint=>{
            let input=node.inputs[0];
            if program.nodes[input].ty==Type::Vec2 {
                footprint(node,source);
            } else {
                writeln!(source,"var footprint=0.0;if fp>0.0 {{let dx=n{input}(p+vec2<f32>(fp*0.5,0.0),0.0,env).x-n{input}(p-vec2<f32>(fp*0.5,0.0),0.0,env).x;let dy=n{input}(p+vec2<f32>(0.0,fp*0.5),0.0,env).x-n{input}(p-vec2<f32>(0.0,fp*0.5),0.0,env).x;footprint=abs(dx)+abs(dy);}}").unwrap();
            }
            "vec4<f32>(footprint)".into()
        },
        SampleAt=>{
            let input=node.inputs[1];let src=node.inputs[0];
            writeln!(source,"var transformed_fp=0.0;if fp>0.0 {{let center=n{input}(p,0.0,env).xy;let dx=n{input}(p+vec2<f32>(fp,0.0),0.0,env).xy;let dy=n{input}(p+vec2<f32>(0.0,fp),0.0,env).xy;transformed_fp=max(length(dx-center),length(dy-center));}}").unwrap();
            format!("n{src}(b.xy,transformed_fp,env)")
        },
        Over=>"vec4<f32>(a.xyz+b.xyz*(1.0-clamp(a.w,0.0,1.0)),clamp(a.w,0.0,1.0)+clamp(b.w,0.0,1.0)*(1.0-clamp(a.w,0.0,1.0)))".into(),
        Mask|Opacity=>"a*clamp(b.x,0.0,1.0)".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_calls_emit_one_temporary_and_unreachable_functions_are_omitted() {
        let program=compile(&parse("syar 1;fn field(v:float)->float{var x=v;for i in 0..4096{x=cos(x);}return x;}fn unused()->float{return 0.3;}let v=field(x(p));output main=rgba(v+v,v*v,v);output other=rgba(unused(),0,0);").unwrap()).unwrap();
        let source = generate(&program, "main").unwrap();
        assert_eq!(source.matches("clean(f0(p,fp").count(), 1);
        assert!(source.contains("i0<4096u"));
        assert!(!source.contains("fn f1("));
        assert!(source.len() < 20_000, "loop must stay structured");
    }
}
