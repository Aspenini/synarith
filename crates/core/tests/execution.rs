use synarith_core::*;

fn program(source: &str) -> Program {
    compile(&parse(source).unwrap()).unwrap()
}

#[test]
fn expressions_share_values_and_sampling_contexts_stay_separate() {
    let p = program("syar 1;let v=sin(x(p));output main=rgba(v+v,v*v,v);");
    let plan = ExecutionPlan::for_output(&p, "main").unwrap();
    let block = plan.block(p.output("main").unwrap()).unwrap();
    assert_eq!(
        block
            .nodes
            .iter()
            .filter(|n| matches!(p.nodes[n.node].kind, NodeKind::Builtin(Op::Sin)))
            .count(),
        1
    );
    for (register, node) in block.nodes.iter().enumerate() {
        assert!(node.inputs.iter().flatten().all(|input| *input < register));
    }
    let p = program(
        "syar 1;let field=rgba(x(p),y(p),0);output main=sample_at(field,p*2.0)+sample_at(field,p*3.0);",
    );
    let plan = ExecutionPlan::compile(&p).unwrap();
    let field = p.named_nodes["field"];
    assert!(plan.block(field).is_some());
    let block = plan.block(p.output("main").unwrap()).unwrap();
    assert!(!block.nodes.iter().any(|n| n.node == field));
    for n in &block.nodes {
        if matches!(p.nodes[n.node].kind, NodeKind::Builtin(Op::SampleAt)) {
            assert_eq!(n.inputs[0], None);
            assert!(plan.block(p.nodes[n.node].inputs[1]).is_some());
        }
    }
}

#[test]
fn reachability_prunes_other_outputs_and_retains_structured_loops() {
    let p = program(
        "syar 1;fn f(v:float)->float{var total=v;for i in 0..4096{total+=0.0001;}return total;}fn unused(v:float)->float{return cos(v);}output main=rgba(f(x(p)),0,0);output other=rgba(unused(x(p)),0,0);",
    );
    let plan = ExecutionPlan::for_output(&p, "main").unwrap();
    assert_eq!(plan.functions.len(), 1);
    let index = *plan.functions.first().unwrap();
    assert_eq!(p.functions[index].name, "f");
    assert!(
        p.functions[index]
            .body
            .iter()
            .any(|s| matches!(s, Instruction::For { end: 4096, .. }))
    );
    assert!(plan.instruction_count < 60);
    let all = ExecutionPlan::compile(&p).unwrap();
    assert_eq!(all.functions.len(), 2);
    assert!(ExecutionPlan::for_output(&p, "missing").is_err());
}

#[test]
fn schedule_memory_is_bounded_independently_of_work_per_sample() {
    let mut source = String::from("syar 1;let base=rgba(0.1,0.2,0.3);let extra=rgba(0.2,0.3,0.4);");
    let mut last = "base".to_owned();
    for i in 0..88 {
        source.push_str(&format!("let v{i}=sin({last});"));
        last = format!("v{i}");
    }
    for i in 0..3000 {
        source.push_str(&format!("output o{i}={last}+extra;"));
    }
    let p = program(&source);
    assert!(
        ExecutionPlan::compile(&p)
            .unwrap_err()
            .contains("scheduled instructions")
    );
    assert!(ExecutionPlan::for_output(&p, "o0").is_ok());
}
