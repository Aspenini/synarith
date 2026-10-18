//! Source-defined standard library, explicitly selected with `use std;`.
use crate::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::OnceLock,
};

pub use synarith_stdlib::standard_library_source;

fn expression(e: &Expr, calls: &mut BTreeSet<String>) {
    match &e.kind {
        ExprKind::Call(name, args) => {
            calls.insert(name.clone());
            for a in args {
                expression(&a.value, calls);
            }
        }
        ExprKind::Unary(_, a) => expression(a, calls),
        ExprKind::Binary(_, a, b) => {
            expression(a, calls);
            expression(b, calls);
        }
        _ => (),
    }
}
fn statements(body: &[Statement], calls: &mut BTreeSet<String>) {
    for s in body {
        match &s.kind {
            StatementKind::Binding(_, _, _, e)
            | StatementKind::Assign(_, e)
            | StatementKind::Return(e) => expression(e, calls),
            StatementKind::If(e, a, b) => {
                expression(e, calls);
                statements(a, calls);
                statements(b, calls);
            }
            StatementKind::For(_, a, b, body) | StatementKind::While(a, b, body) => {
                expression(a, calls);
                expression(b, calls);
                statements(body, calls);
            }
            _ => (),
        }
    }
}
fn calls(doc: &Document) -> BTreeSet<String> {
    let mut calls = BTreeSet::new();
    for d in doc.declarations.iter().chain(&doc.outputs) {
        expression(&d.expr, &mut calls);
    }
    for g in &doc.groups {
        for d in &g.body {
            expression(&d.expr, &mut calls);
        }
        expression(&g.result, &mut calls);
    }
    for f in &doc.functions {
        statements(&f.body, &mut calls);
    }
    calls
}
/// Expand an explicit `use std;` into only the required ordinary definitions.
///
/// Preserves document comments and other source text. Returns a self-contained
/// source document with no library directive, suitable for copying or archiving.
/// Local definitions take precedence over library helpers. Unknown operations
/// remain compiler diagnostics; expansion does not silently invent definitions.
///
/// # Errors
/// Returns syntax or source-size diagnostics if the expanded source cannot parse.
pub fn expand_standard_library(doc: &Document) -> Result<String, Diagnostic> {
    let Some(span) = doc.standard_library else {
        return Ok(doc.source.clone());
    };
    let library = library();
    let mut source = doc.source.clone();
    // Equal-length whitespace keeps every original declaration span stable.
    let directive = crate::language::blank_directive(&source[span.start..span.end])?;
    source.replace_range(span.start..span.end, &directive);
    let mut included: BTreeSet<String> = doc
        .functions
        .iter()
        .map(|f| f.name.clone())
        .chain(doc.groups.iter().map(|g| g.name.clone()))
        .collect();
    let mut pending = calls(doc);
    while let Some(name) = pending.pop_first() {
        if included.contains(&name) {
            continue;
        }
        if let Some((span, dependencies)) = library.get(&name) {
            included.insert(name);
            source.push('\n');
            source.push_str(&standard_library_source()[span.start..span.end]);
            source.push('\n');
            pending.extend(
                dependencies
                    .iter()
                    .filter(|n| !included.contains(*n))
                    .cloned(),
            );
        }
    }
    Ok(source)
}
pub(crate) fn expand(doc: &Document) -> Result<Document, Diagnostic> {
    parse(&expand_standard_library(doc)?)
}

type LibraryIndex = BTreeMap<String, (Span, BTreeSet<String>)>;
fn library() -> &'static LibraryIndex {
    static INDEX: OnceLock<LibraryIndex> = OnceLock::new();
    INDEX.get_or_init(|| {
        // Fixed bundled source is validated by the library coverage tests.
        let doc = parse(standard_library_source()).expect("bundled standard library must parse");
        let mut index = BTreeMap::new();
        for f in &doc.functions {
            let mut dependencies = BTreeSet::new();
            statements(&f.body, &mut dependencies);
            index.insert(f.name.clone(), (f.span, dependencies));
        }
        for g in &doc.groups {
            let mut dependencies = BTreeSet::new();
            for d in &g.body {
                expression(&d.expr, &mut dependencies);
            }
            expression(&g.result, &mut dependencies);
            index.insert(g.name.clone(), (g.span, dependencies));
        }
        index
    })
}
