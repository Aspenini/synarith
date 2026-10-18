use synarith_core::*;

#[test]
fn representative_document_and_reserved_function_names() {
    let p = compile(&parse(include_str!("../../../examples/clouds.syar")).unwrap()).unwrap();
    assert_eq!(p.parameters.len(), 4);
    assert!(p.outputs.contains_key("main"));
}
#[test]
fn groups_named_arguments_and_forward_references() {
    let source = "syar 1;use std; group paint(q:vec2, ink:color)->color {let d=circle(q);return fill(d,ink);} let image=paint(ink:srgb(\"#123456\"),q:pos);let pos=p-vec2(0.5,0.5);output main=image;";
    assert!(compile(&parse(source).unwrap()).is_ok());
}
#[test]
fn diagnoses_invalid_documents() {
    for (source, expected) in [
        (
            "syar 2;output main=rgba(0,0,0);",
            "unsupported format version",
        ),
        ("syar 1;use std;output main=missing;", "unknown name"),
        (
            "syar 1;use std;output main=mystery(p);",
            "unknown operation",
        ),
        (
            "syar 1;use std;let a=b;let b=a;output main=rgba(0,0,0);",
            "cyclic reference",
        ),
        (
            "syar 1;use std;group recursive(q:vec2)->color{return recursive(q);}output main=rgba(0,0,0);",
            "recursive group",
        ),
        (
            "syar 1;output main=rgba(sqrt(-1),0,0);",
            "invalid constant mathematical domain",
        ),
        (
            "syar 1;output main=rgba(log(0),0,0);",
            "invalid constant mathematical domain",
        ),
        (
            "syar 1;use std;output main=fill(fbm(p,octaves:2.5));",
            "incorrect group argument type",
        ),
        (
            "syar 1;use std;output main=rgba(mandelbrot(p,128),0,0);",
            "unknown operation",
        ),
        (
            "syar 1;use std;output main=rgba(true,0,0);",
            "incorrect type",
        ),
        ("syar 1;use std;output main=0.5;", "outputs must be colors"),
        (
            "syar 1;use std;output main=rgba(1/0,0,0);",
            "division by zero",
        ),
        (
            "syar 1;use std;output main=rgba(0,0,0); /* missing",
            "unterminated comment",
        ),
        (
            "syar 1;use std;output main=rgba(0,0,0);let x=1;let x=2;",
            "duplicate or reserved",
        ),
        (
            "syar 1;use std;group broken(a:float)->color{return unknown;}output main=rgba(0,0,0);",
            "unknown name",
        ),
    ] {
        let err = parse(source).and_then(|doc| compile(&doc)).unwrap_err();
        assert!(
            err.message.contains(expected),
            "expected {expected}, got {err}"
        );
        assert!(err.line >= 1 && err.column >= 1);
    }
}
#[test]
fn source_preserving_edits_and_metadata() {
    let source = "syar 1;use std;\n// keep this\nparam size: float = 0.3; // trailing\noutput main=fill(circle(p,size));\n";
    let doc = parse(source).unwrap();
    let edited = doc
        .replace_expression(doc.parameters[0].default.span, "0.4")
        .unwrap();
    assert_eq!(edited.source, source.replace("0.3", "0.4"));
    let metadata = std::collections::BTreeMap::from([(
        "positions".into(),
        serde_json::json!({"shape":[1,2]}),
    )]);
    let new_source = edited.with_editor(&metadata);
    let reopened = parse(&new_source).unwrap();
    assert_eq!(reopened.editor, metadata);
    assert_eq!(reopened.with_editor(&metadata), new_source);
    assert_eq!(format_source(&new_source).unwrap(), new_source);
}

#[test]
fn comments_inside_replaced_expressions_survive() {
    let doc = parse(
        "syar 1;use std;param value:float=(/* artist note */ 0.5);output main=rgba(value,0,0);",
    )
    .unwrap();
    let edited = doc
        .replace_expression(doc.parameters[0].default.span, "0.7")
        .unwrap();
    assert!(edited.source.contains("/* artist note */"));
    assert_eq!(compile(&edited).unwrap().defaults()[0].data[0], 0.7);
}
#[test]
fn seed_preserves_all_u32_bits() {
    for seed in [0, 16_777_217, u32::MAX] {
        let doc = parse(&format!(
            "syar 1;use std;seed {seed};output main=rgba(0,0,0);"
        ))
        .unwrap();
        assert_eq!(doc.seed, seed);
    }
}
#[test]
fn unicode_comments_and_strings_have_source_positions() {
    let source = "syar 1;use std;\n// café 🌀\noutput main=unknown;";
    let error = compile(&parse(source).unwrap()).unwrap_err();
    assert_eq!((error.line, error.column), (3, 13));
}
#[test]
fn registry_is_unique_and_documented() {
    let ops = operations();
    let mut seen = std::collections::BTreeSet::new();
    assert!(operation("mandelbrot").is_none());
    for op in ops {
        assert!(seen.insert(op.name));
        assert!(!op.description.is_empty());
        assert!(op.inputs.len() <= 7);
    }
}
#[test]
fn numeric_promotion_and_parameter_contract() {
    let p = compile(
        &parse("syar 1;use std;param a:float=1;let v=1.5+1;output main=rgba(v,0,0);").unwrap(),
    )
    .unwrap();
    assert_eq!(p.nodes[p.named_nodes["v"]].ty, Type::Float);
    assert_eq!(p.defaults()[0].ty, Type::Float);
    assert!(p.check_parameters(&[Value::float(f32::NAN)]).is_err());
}
#[test]
fn nesting_is_bounded_without_stack_overflow() {
    let source = format!(
        "syar 1;use std;output main=rgba({}0{},0,0);",
        "sin(".repeat(200),
        ")".repeat(200)
    );
    assert!(parse(&source).unwrap_err().message.contains("nesting"));
}

#[test]
fn reusable_groups_are_validated_with_dynamic_inputs() {
    let source = "syar 1;use std;group primitive(q:vec2,n:int)->float{let d=polygon(q,sides:n);let a=box(q,size:q);return union(d,a);}output main=rgba(0,0,0);";
    assert!(compile(&parse(source).unwrap()).is_ok());
    let source = "syar 1;use std;group local(q:vec2)->color{let paint=fill(d);let d=circle(q);return paint;}output main=local(p);";
    assert!(compile(&parse(source).unwrap()).is_ok());
    // Library controls are mathematical values clamped by the source algorithm.
    let source = "syar 1;use std;output main=rgba(periodic_noise(p,vec2(0,4)),0,0);";
    assert!(compile(&parse(source).unwrap()).is_ok());
}
