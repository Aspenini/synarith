use synarith_core::*;

#[test]
fn algorithms_are_source_definitions_and_require_explicit_opt_in() {
    for name in [
        "noise",
        "gradient_noise",
        "fbm",
        "cellular",
        "circle",
        "polygon",
        "path",
        "blur",
        "normal",
        "mandelbrot",
    ] {
        assert!(operation(name).is_none(), "{name} must not be an opcode");
        let error =
            compile(&parse(&format!("syar 1;output main={name}(p);")).unwrap()).unwrap_err();
        assert!(error.message.contains("unknown operation"), "{error}");
    }
    let source = format!("{}\noutput main=rgba(0,0,0);", standard_library_source());
    compile(&parse(&source).unwrap()).unwrap(); // Validate every body and default.
    let custom =
        "syar 1;fn circle(q:vec2)->float{return length(q)-0.3;}output main=rgba(circle(p),0,0);";
    assert!(compile(&parse(custom).unwrap()).is_ok());
}

#[test]
fn expansion_is_standalone_minimal_and_retains_source_trivia() {
    let source = "syar 1;\nuse /* keep inside */ std; // keep this\n// café\noutput main=fill(circle(p,0.2));\n";
    let doc = parse(source).unwrap();
    assert_eq!(doc.source, source);
    assert_eq!(format_source(source).unwrap(), source);
    let expanded = expand_standard_library(&doc).unwrap();
    assert!(expanded.contains("// keep this\n// café\noutput main=fill(circle(p,0.2));"));
    assert!(expanded.contains("/* keep inside */"));
    assert!(expanded.contains("fn circle("));
    assert!(!expanded.contains("fn std_noise("));
    assert!(!expanded.contains("fn mandelbrot("));
    let standalone = parse(&expanded).unwrap();
    assert!(standalone.standard_library.is_none());
    assert_eq!(expand_standard_library(&standalone).unwrap(), expanded);
    let direct = compile(&doc).unwrap();
    let embedded = compile(&standalone).unwrap();
    assert_eq!(direct.nodes.len(), embedded.nodes.len());
    assert_eq!(direct.estimated_cost, embedded.estimated_cost);
    // Locally supplied definitions replace source-library helpers.
    let override_doc =
        parse("syar 1;use std;fn circle(q:vec2)->float{return x(q);}output main=fill(circle(p));")
            .unwrap();
    let expanded = expand_standard_library(&override_doc).unwrap();
    assert_eq!(expanded.matches("fn circle(").count(), 1);
    compile(&override_doc).unwrap();
}

#[test]
fn defaults_imports_and_sampling_have_checked_diagnostics() {
    for source in [
        "syar 1;use filesystem;output main=rgba(0,0,0);",
        "syar 1;use std;use std;output main=rgba(0,0,0);",
        "syar 1;fn f(a:int=1.5)->float{return float(a);}output main=rgba(0,0,0);",
        "syar 1;group f(a:float=p)->float{return a;}output main=rgba(0,0,0);",
        "syar 1;output main=rgba(footprint(rgba(1,1,1)),0,0);",
        "syar 1;output main=rgba(hash(p,0.5),0,0);",
    ] {
        let error = parse(source).and_then(|d| compile(&d)).unwrap_err();
        assert!(error.line > 0 && error.column > 0);
    }
    let doc = parse("syar 1;fn f(a:float=1)->float{return a;}group g(a:float=0.5)->float{return a;}output main=rgba(f(),g(),0);").unwrap();
    compile(&doc).unwrap();
    let doc = parse("syar 1;use std;\noutput main=fill(circle(p), color: true);").unwrap();
    assert_eq!(compile(&doc).unwrap_err().line, 2);
}
