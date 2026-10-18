use synarith_core::*;

#[test]
fn functions_preserve_source_and_compile_compact_structured_loops() {
    let source = r#"syar 1;
// This algorithm is supplied by the document.
fn sum(n: int) -> float {
    var result: float = 0.0;
    let steps = 1024;
    for index in 0..steps {
        if index >= n { break; }
        result += float(index); // keep this comment
    }
    return result;
}
output main = rgba(sum(n: 8), 0, 0);
"#;
    let doc = parse(source).unwrap();
    assert_eq!(doc.source, source);
    assert_eq!(format_source(source).unwrap(), source);
    assert_eq!(doc.functions.len(), 1);
    let program = compile(&doc).unwrap();
    assert_eq!(program.functions.len(), 1);
    assert!(program.nodes.len() < 40, "loop must not be unrolled");
    assert!(program.estimated_cost >= 1024);
    let StatementKind::Binding(_, _, _, initializer) = &doc.functions[0].body[0].kind else {
        panic!()
    };
    let edited = doc.replace_expression(initializer.span, "1.0").unwrap();
    assert!(edited.source.contains("// keep this comment"));
    assert!(edited.source.contains("result: float = 1.0"));
    compile(&edited).unwrap();
}

#[test]
fn invalid_algorithms_have_source_diagnostics_even_if_unused() {
    for (body, expected) in [
        ("var z:float=0; z=true;return z;", "assignment changes"),
        ("let z=1;z=2;return float(z);", "immutable"),
        ("break;return 0.0;", "enclosing loop"),
        ("continue;return 0.0;", "enclosing loop"),
        ("for i in 0..t {} return 0.0;", "compile-time"),
        ("for i in 0..4097 {} return 0.0;", "0..4096"),
        ("for i in 4..2 {} return 0.0;", "precede"),
        ("for i in 0..4 {i=2;} return 0.0;", "immutable"),
        ("while true limit -1 {} return 0.0;", "0..4096"),
        ("if 1 {return 0.0;}return 1.0;", "must be bool"),
        ("if t>0 {return 0.0;}", "every path"),
        (
            "if true {let hidden=1;}return float(hidden);",
            "unknown name",
        ),
        ("return vec2(1,0);", "must return float"),
        ("return f();", "recursive function"),
        ("var p=1;return 0.0;", "reserved"),
        ("var z=1;var z=2;return 0.0;", "duplicate"),
        ("return 1.0;return 0.0;", "unreachable"),
        (
            "for a in 0..4096 {for b in 0..4096 {var z=a+b;}}return 0.0;",
            "evaluation cost",
        ),
    ] {
        let source = format!("syar 1;\nfn f()->float {{ {body} }}\noutput main=rgba(0,0,0);");
        let doc = parse(&source).unwrap_or_else(|e| panic!("{body}: {e}"));
        let error = compile(&doc).expect_err(body);
        assert!(
            error.message.contains(expected),
            "{body}: {error}, expected {expected}"
        );
        assert!(error.line >= 2);
        assert!(error.span.start <= error.span.end && error.span.end <= source.len());
    }
    for source in [
        "syar 1;fn f()->float{return g();}fn g()->float{return f();}output main=rgba(0,0,0);",
        "syar 1;fn sin()->float{return 0.0;}output main=rgba(0,0,0);",
        "syar 1;fn f(a:float,a:int)->float{return 0.0;}output main=rgba(0,0,0);",
        "syar 1;fn f()->float{return 0.0;}let f=1;output main=rgba(0,0,0);",
        "syar 1;fn f()->float{return 0.0;}output main=rgba(f(1),0,0);",
        "syar 1;fn f(a:int)->float{return float(a);}output main=rgba(f(1.5),0,0);",
        "syar 1;fn f(a:int)->float{return float(a);}output main=rgba(f(),0,0);",
        "syar 1;fn f(a:int)->float{return float(a);}output main=rgba(f(a:1,2),0,0);",
    ] {
        assert!(compile(&parse(source).unwrap()).is_err(), "{source}");
    }
}

#[test]
fn malicious_statement_nesting_is_bounded() {
    let source = format!(
        "syar 1;fn f()->float{{{}{{return 0.0;}}return 1.0;}}output main=rgba(0,0,0);",
        "if true {} else ".repeat(2000)
    );
    assert!(parse(&source).unwrap_err().message.contains("nesting"));
    let functions = (0..120)
        .map(|i| format!("fn f{i}()->float{{return f{}();}}", i + 1))
        .collect::<String>();
    let source =
        format!("syar 1;{functions}fn f120()->float{{return 1.0;}}output main=rgba(f0(),0,0);");
    assert!(
        compile(&parse(&source).unwrap())
            .unwrap_err()
            .message
            .contains("nesting")
    );
}

#[test]
fn shader_size_and_frame_storage_are_bounded_before_rendering() {
    let definitions = (0..129)
        .map(|i| format!("fn f{i}()->float{{return 0.0;}}"))
        .collect::<String>();
    let source = format!("syar 1;{definitions}output main=rgba(0,0,0);");
    assert!(
        compile(&parse(&source).unwrap())
            .unwrap_err()
            .message
            .contains("128 functions")
    );
    let statements = "if flag {} ".repeat(4097);
    let source =
        format!("syar 1;fn f(flag:bool)->float{{{statements}return 0.0;}}output main=rgba(0,0,0);");
    assert!(
        compile(&parse(&source).unwrap())
            .unwrap_err()
            .message
            .contains("4096 function instructions")
    );
    let locals = (0..129)
        .map(|i| format!("var a{i}=0.0;"))
        .collect::<String>();
    let source = format!("syar 1;fn f()->float{{{locals}return 0.0;}}output main=rgba(0,0,0);");
    assert!(
        compile(&parse(&source).unwrap())
            .unwrap_err()
            .message
            .contains("128 local slots")
    );
}
