use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt};

/// Maximum accepted UTF-8 document size: 4 MiB.
pub const MAX_SOURCE_BYTES: usize = 4 * 1024 * 1024;

/// Half-open UTF-8 byte range in the original document.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    /// Inclusive UTF-8 byte offset.
    pub start: usize,
    /// Exclusive UTF-8 byte offset.
    pub end: usize,
}

/// A source-position error suitable for display and editor integration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// Human-readable explanation.
    pub message: String,
    /// Affected UTF-8 byte range.
    pub span: Span,
    /// One-based source line.
    pub line: usize,
    /// One-based character column within the line.
    pub column: usize,
}
impl Diagnostic {
    /// Create a diagnostic and derive its line/column from a source span.
    ///
    /// # Panics
    /// Panics if `span.start`, after clamping to source length, splits a UTF-8
    /// character. Parser-provided spans always lie on character boundaries.
    pub fn new(source: &str, span: Span, message: impl Into<String>) -> Self {
        let prefix = &source[..span.start.min(source.len())];
        Self {
            message: message.into(),
            span,
            line: prefix.bytes().filter(|b| *b == b'\n').count() + 1,
            column: prefix.rsplit('\n').next().unwrap_or("").chars().count() + 1,
        }
    }
}
impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: {}", self.line, self.column, self.message)
    }
}
impl std::error::Error for Diagnostic {}

/// A value type in the Synarith language.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Type {
    /// A scalar f32 value.
    Float,
    /// An integral scalar represented exactly within ±16777216.
    Int,
    /// A boolean encoded as zero or one.
    Bool,
    /// A two-dimensional vector.
    Vec2,
    /// Linear premultiplied RGBA.
    Color,
}
impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Float => "float",
            Self::Int => "int",
            Self::Bool => "bool",
            Self::Vec2 => "vec2",
            Self::Color => "color",
        })
    }
}
/// A parsed expression with its original source range.
#[derive(Clone, Debug)]
pub struct Expr {
    /// Literal, reference, operator, or call.
    pub kind: ExprKind,
    /// Byte range of the complete expression.
    pub span: Span,
}
/// Parsed expression syntax before name resolution or type checking.
#[derive(Clone, Debug)]
pub enum ExprKind {
    /// Numeric literal and whether it has integer syntax.
    Number(f32, bool),
    /// Boolean literal.
    Bool(bool),
    /// JSON-decoded string, allowed in color literals and metadata.
    String(String),
    /// Reference to a field, parameter, or built-in variable.
    Name(String),
    /// Operator spelling and its operand.
    Unary(String, Box<Expr>),
    /// Operator spelling and its left/right operands.
    Binary(String, Box<Expr>, Box<Expr>),
    /// Function/group name and ordered arguments.
    Call(String, Vec<Argument>),
}
/// A positional or named function/group argument.
#[derive(Clone, Debug)]
pub struct Argument {
    /// Named argument label, or `None` for a positional argument.
    pub name: Option<String>,
    /// Argument expression.
    pub value: Expr,
}
/// A named `let` field or `output` declaration.
#[derive(Clone, Debug)]
pub struct Declaration {
    /// Declared field or output name.
    pub name: String,
    /// Expression assigned to the name.
    pub expr: Expr,
    /// Byte range of the declaration.
    pub span: Span,
}
/// A typed, externally overridable parameter declaration.
#[derive(Clone, Debug)]
pub struct Parameter {
    /// Exposed parameter name.
    pub name: String,
    /// Declared parameter type.
    pub ty: Type,
    /// Constant default expression, checked during compilation.
    pub default: Expr,
    /// Byte range of the parameter declaration.
    pub span: Span,
}
/// A nonrecursive reusable group with typed inputs and one result.
#[derive(Clone, Debug)]
pub struct Group {
    /// Reusable group name.
    pub name: String,
    /// Ordered argument names and declared types.
    pub args: Vec<(String, Type)>,
    /// Declared return type.
    pub ty: Type,
    /// Local named expressions.
    pub body: Vec<Declaration>,
    /// Returned expression.
    pub result: Expr,
    /// Byte offset of the return statement.
    pub return_start: usize,
    /// Byte range of the complete group declaration.
    pub span: Span,
}
/// Parsed source and declarations, preserving comments through source spans.
///
/// Parsing checks syntax; [`crate::compile`] performs semantic validation.
/// Treat source text and its recorded spans as a consistent snapshot.
#[derive(Clone, Debug)]
pub struct Document {
    /// Original UTF-8 source, including whitespace and comments.
    pub source: String,
    /// Exposed parameters in declaration order.
    pub parameters: Vec<Parameter>,
    /// Top-level named fields.
    pub declarations: Vec<Declaration>,
    /// Named color outputs in source order.
    pub outputs: Vec<Declaration>,
    /// Reusable typed groups.
    pub groups: Vec<Group>,
    /// Document-space `[x, y, width, height]`; defaults to the unit square.
    pub viewbox: [f32; 4],
    /// Exact 32-bit default seed.
    pub seed: u32,
    /// Ignored legacy JSON layout metadata, retained for round trips.
    pub editor: BTreeMap<String, serde_json::Value>,
    /// Byte range of an existing metadata declaration, if present.
    pub editor_span: Option<Span>,
}
impl Document {
    /// Replace only an expression; surrounding comments and layout remain byte-for-byte intact.
    ///
    /// Reparse the result and call [`crate::compile`] for semantic validation.
    ///
    /// # Errors
    /// Rejects invalid UTF-8 byte ranges or edits that produce invalid syntax.
    ///
    /// ```
    /// use synarith_core::parse;
    /// let document = parse("syar 1;param gain:float=0.2; // keep me\noutput main=rgba(gain,0,0);")?;
    /// let span = document.parameters[0].default.span;
    /// let edited = document.replace_expression(span, "0.8")?;
    /// assert!(edited.source.contains("// keep me"));
    /// assert!(edited.source.contains("gain:float=0.8"));
    /// # Ok::<(), synarith_core::Diagnostic>(())
    /// ```
    pub fn replace_expression(&self, span: Span, expression: &str) -> Result<Self, Diagnostic> {
        let mut source = self.source.clone();
        if span.start > span.end
            || !source.is_char_boundary(span.start)
            || !source.is_char_boundary(span.end)
        {
            return Err(Diagnostic::new(
                &source,
                Span::default(),
                "invalid edit range",
            ));
        }
        let replacement = preserve_expression_comments(&source[span.start..span.end], expression)?;
        source.replace_range(span.start..span.end, &replacement);
        parse(&source)
    }
    /// Replace or append ignored legacy layout metadata, returning updated source.
    ///
    /// Other source bytes are retained. Reparse the result to refresh spans.
    pub fn with_editor(&self, editor: &BTreeMap<String, serde_json::Value>) -> String {
        let json = serde_json::to_string(editor).expect("JSON editor metadata");
        let declaration = format!("editor {};", serde_json::to_string(&json).unwrap());
        let mut source = self.source.clone();
        if let Some(span) = self.editor_span {
            source.replace_range(span.start..span.end, &declaration);
        } else {
            source.push_str(&format!("\n{declaration}\n"));
        }
        source
    }
}

#[derive(Clone, Debug, PartialEq)]
enum TokenKind {
    Ident(String),
    Number(f32, bool),
    String(String),
    Symbol(String),
    End,
}
#[derive(Clone, Debug)]
struct Token {
    kind: TokenKind,
    span: Span,
}

fn lex(source: &str) -> Result<Vec<Token>, Diagnostic> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if source[i..].starts_with("//") {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if source[i..].starts_with("/*") {
            let start = i;
            i += 2;
            while i + 1 < bytes.len() && &bytes[i..i + 2] != b"*/" {
                i += 1;
            }
            if i + 1 == bytes.len() || i == bytes.len() {
                return Err(Diagnostic::new(
                    source,
                    Span {
                        start,
                        end: bytes.len(),
                    },
                    "unterminated comment",
                ));
            }
            i += 2;
            continue;
        }
        let start = i;
        let kind = if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            TokenKind::Ident(source[start..i].into())
        } else if bytes[i].is_ascii_digit()
            || (bytes[i] == b'.' && bytes.get(i + 1).is_some_and(u8::is_ascii_digit))
        {
            let mut integer = true;
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                if bytes[i] == b'.' {
                    integer = false;
                }
                i += 1;
            }
            if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
                integer = false;
                i += 1;
                if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
                    i += 1;
                }
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
            }
            let span = Span { start, end: i };
            let value: f32 = source[start..i]
                .parse()
                .map_err(|_| Diagnostic::new(source, span, "invalid number"))?;
            if !value.is_finite() {
                return Err(Diagnostic::new(source, span, "numbers must be finite"));
            }
            TokenKind::Number(value, integer)
        } else if bytes[i] == b'"' {
            i += 1;
            while i < bytes.len() && bytes[i] != b'"' {
                if bytes[i] == b'\\' {
                    i += 1;
                }
                i += 1;
            }
            if i >= bytes.len() {
                return Err(Diagnostic::new(
                    source,
                    Span {
                        start,
                        end: bytes.len(),
                    },
                    "unterminated string",
                ));
            }
            i += 1;
            TokenKind::String(serde_json::from_str(&source[start..i]).map_err(|_| {
                Diagnostic::new(source, Span { start, end: i }, "invalid string escape")
            })?)
        } else {
            if !bytes[i].is_ascii() {
                return Err(Diagnostic::new(
                    source,
                    Span { start, end: start },
                    "expected an ASCII identifier or punctuation",
                ));
            }
            i += 1;
            if i < bytes.len()
                && matches!(
                    &source[start..i + 1],
                    "->" | "==" | "!=" | "<=" | ">=" | "&&" | "||"
                )
            {
                i += 1;
            }
            let symbol = &source[start..i];
            if !matches!(
                symbol,
                "(" | ")"
                    | "{"
                    | "}"
                    | ","
                    | ":"
                    | ";"
                    | "="
                    | "+"
                    | "-"
                    | "*"
                    | "/"
                    | "%"
                    | "!"
                    | "<"
                    | ">"
                    | "->"
                    | "=="
                    | "!="
                    | "<="
                    | ">="
                    | "&&"
                    | "||"
            ) {
                return Err(Diagnostic::new(
                    source,
                    Span { start, end: i },
                    format!("unexpected character {symbol}"),
                ));
            }
            TokenKind::Symbol(symbol.into())
        };
        tokens.push(Token {
            kind,
            span: Span { start, end: i },
        });
    }
    tokens.push(Token {
        kind: TokenKind::End,
        span: Span {
            start: source.len(),
            end: source.len(),
        },
    });
    Ok(tokens)
}

struct Parser<'a> {
    source: &'a str,
    tokens: Vec<Token>,
    index: usize,
    depth: usize,
}
impl<'a> Parser<'a> {
    fn token(&self) -> &Token {
        &self.tokens[self.index]
    }
    fn error(&self, message: impl Into<String>) -> Diagnostic {
        Diagnostic::new(self.source, self.token().span, message)
    }
    fn take(&mut self) -> Token {
        let t = self.token().clone();
        if t.kind != TokenKind::End {
            self.index += 1;
        }
        t
    }
    fn eat(&mut self, symbol: &str) -> bool {
        if self.token().kind == TokenKind::Symbol(symbol.into()) {
            self.take();
            true
        } else {
            false
        }
    }
    fn expect(&mut self, symbol: &str) -> Result<(), Diagnostic> {
        if self.eat(symbol) {
            Ok(())
        } else {
            Err(self.error(format!("expected '{symbol}'")))
        }
    }
    fn ident(&mut self) -> Result<String, Diagnostic> {
        match self.take().kind {
            TokenKind::Ident(s) => Ok(s),
            _ => Err(self.error("expected identifier")),
        }
    }
    fn ty(&mut self) -> Result<Type, Diagnostic> {
        match self.ident()?.as_str() {
            "float" => Ok(Type::Float),
            "int" => Ok(Type::Int),
            "bool" => Ok(Type::Bool),
            "vec2" => Ok(Type::Vec2),
            "color" => Ok(Type::Color),
            _ => Err(self.error("expected float, int, bool, vec2, or color")),
        }
    }
    fn expr(&mut self, min: u8) -> Result<Expr, Diagnostic> {
        self.depth += 1;
        if self.depth > 128 {
            return Err(self.error("expression nesting exceeds 128"));
        }
        let token = self.take();
        let start = token.span.start;
        let mut left = match token.kind {
            TokenKind::Number(v, int) => Expr {
                kind: ExprKind::Number(v, int),
                span: token.span,
            },
            TokenKind::String(s) => Expr {
                kind: ExprKind::String(s),
                span: token.span,
            },
            TokenKind::Ident(name) if name == "true" || name == "false" => Expr {
                kind: ExprKind::Bool(name == "true"),
                span: token.span,
            },
            TokenKind::Ident(name) => {
                if self.eat("(") {
                    let mut args = Vec::new();
                    if !self.eat(")") {
                        loop {
                            let named = matches!(self.token().kind, TokenKind::Ident(_))
                                && self
                                    .tokens
                                    .get(self.index + 1)
                                    .is_some_and(|t| t.kind == TokenKind::Symbol(":".into()));
                            let name = if named {
                                let n = self.ident()?;
                                self.expect(":")?;
                                Some(n)
                            } else {
                                None
                            };
                            args.push(Argument {
                                name,
                                value: self.expr(0)?,
                            });
                            if self.eat(")") {
                                break;
                            }
                            self.expect(",")?;
                            if self.eat(")") {
                                break;
                            }
                        }
                    }
                    Expr {
                        kind: ExprKind::Call(name, args),
                        span: Span {
                            start,
                            end: self.tokens[self.index - 1].span.end,
                        },
                    }
                } else {
                    Expr {
                        kind: ExprKind::Name(name),
                        span: token.span,
                    }
                }
            }
            TokenKind::Symbol(s) if s == "-" || s == "+" || s == "!" => {
                let expr = self.expr(13)?;
                let end = expr.span.end;
                Expr {
                    kind: ExprKind::Unary(s, Box::new(expr)),
                    span: Span { start, end },
                }
            }
            TokenKind::Symbol(s) if s == "(" => {
                let mut e = self.expr(0)?;
                self.expect(")")?;
                e.span = Span {
                    start,
                    end: self.tokens[self.index - 1].span.end,
                };
                e
            }
            _ => {
                return Err(Diagnostic::new(
                    self.source,
                    token.span,
                    "expected expression",
                ));
            }
        };
        while let TokenKind::Symbol(op) = &self.token().kind {
            let precedence = match op.as_str() {
                "||" => 1,
                "&&" => 3,
                "==" | "!=" => 5,
                "<" | ">" | "<=" | ">=" => 7,
                "+" | "-" => 9,
                "*" | "/" | "%" => 11,
                _ => break,
            };
            if precedence < min {
                break;
            }
            let op = op.clone();
            self.take();
            let right = self.expr(precedence + 1)?;
            let end = right.span.end;
            left = Expr {
                kind: ExprKind::Binary(op, Box::new(left), Box::new(right)),
                span: Span { start, end },
            };
        }
        self.depth -= 1;
        Ok(left)
    }
    fn declaration(&mut self, start: usize) -> Result<Declaration, Diagnostic> {
        let name = self.ident()?;
        self.expect("=")?;
        let expr = self.expr(0)?;
        self.expect(";")?;
        Ok(Declaration {
            name,
            expr,
            span: Span {
                start,
                end: self.tokens[self.index - 1].span.end,
            },
        })
    }
    fn signed_number(&mut self) -> Result<f32, Diagnostic> {
        let sign = if self.eat("-") { -1.0 } else { 1.0 };
        match self.take().kind {
            TokenKind::Number(v, _) => Ok(sign * v),
            _ => Err(self.error("expected numeric literal")),
        }
    }
}

/// Parse a version-1 `.syar` document while retaining its original source.
///
/// # Errors
/// Reports invalid headers/versions, malformed tokens or declarations, duplicate
/// view-box/seed/metadata declarations, oversized source, or excessive nesting.
/// Name resolution, graph cycles, and expression types require [`crate::compile`].
///
/// ```
/// let source = "syar 1; // retained comment\noutput main=rgba(1,0,0);";
/// let document = synarith_core::parse(source)?;
/// assert_eq!(document.source, source);
/// # Ok::<(), synarith_core::Diagnostic>(())
/// ```
pub fn parse(source: &str) -> Result<Document, Diagnostic> {
    if source.len() > MAX_SOURCE_BYTES {
        return Err(Diagnostic::new(
            "",
            Span::default(),
            "document exceeds 4 MiB",
        ));
    }
    let mut p = Parser {
        source,
        tokens: lex(source)?,
        index: 0,
        depth: 0,
    };
    if p.ident()? != "syar" {
        return Err(p.error("expected 'syar 1;' header"));
    }
    match p.take().kind {
        TokenKind::Number(1.0, true) => (),
        _ => return Err(p.error("unsupported format version; expected 1")),
    }
    p.expect(";")?;
    let mut doc = Document {
        source: source.into(),
        parameters: vec![],
        declarations: vec![],
        outputs: vec![],
        groups: vec![],
        viewbox: [0., 0., 1., 1.],
        seed: 0,
        editor: BTreeMap::new(),
        editor_span: None,
    };
    let mut has_seed = false;
    let mut has_viewbox = false;
    while p.token().kind != TokenKind::End {
        let start = p.token().span.start;
        match p.ident()?.as_str() {
            "param" => {
                let name = p.ident()?;
                p.expect(":")?;
                let ty = p.ty()?;
                p.expect("=")?;
                let default = p.expr(0)?;
                p.expect(";")?;
                doc.parameters.push(Parameter {
                    name,
                    ty,
                    default,
                    span: Span {
                        start,
                        end: p.tokens[p.index - 1].span.end,
                    },
                });
            }
            "let" => doc.declarations.push(p.declaration(start)?),
            "output" => doc.outputs.push(p.declaration(start)?),
            "seed" => {
                if has_seed {
                    return Err(p.error("duplicate seed"));
                }
                has_seed = true;
                let token = p.take();
                if !matches!(token.kind, TokenKind::Number(_, true)) {
                    return Err(p.error("seed must be an unsigned 32-bit integer"));
                }
                doc.seed = source[token.span.start..token.span.end]
                    .parse::<u32>()
                    .map_err(|_| p.error("seed must be an unsigned 32-bit integer"))?;
                p.expect(";")?;
            }
            "viewbox" => {
                if has_viewbox {
                    return Err(p.error("duplicate viewbox"));
                }
                has_viewbox = true;
                p.expect("(")?;
                for j in 0..4 {
                    doc.viewbox[j] = p.signed_number()?;
                    if j < 3 {
                        p.expect(",")?;
                    }
                }
                p.expect(")")?;
                p.expect(";")?;
                if doc.viewbox[2] <= 0. || doc.viewbox[3] <= 0. {
                    return Err(p.error("viewbox dimensions must be positive"));
                }
                if !(doc.viewbox[0] + doc.viewbox[2]).is_finite()
                    || !(doc.viewbox[1] + doc.viewbox[3]).is_finite()
                {
                    return Err(p.error("viewbox extent must be finite"));
                }
            }
            "editor" => {
                if doc.editor_span.is_some() {
                    return Err(p.error("duplicate editor metadata"));
                }
                let TokenKind::String(json) = p.take().kind else {
                    return Err(p.error("editor metadata must be a JSON string"));
                };
                doc.editor = serde_json::from_str(&json)
                    .map_err(|_| p.error("invalid editor JSON object"))?;
                p.expect(";")?;
                doc.editor_span = Some(Span {
                    start,
                    end: p.tokens[p.index - 1].span.end,
                });
            }
            "group" => {
                let name = p.ident()?;
                p.expect("(")?;
                let mut args = vec![];
                if !p.eat(")") {
                    loop {
                        let n = p.ident()?;
                        p.expect(":")?;
                        let ty = p.ty()?;
                        args.push((n, ty));
                        if p.eat(")") {
                            break;
                        }
                        p.expect(",")?;
                    }
                }
                p.expect("->")?;
                let ty = p.ty()?;
                p.expect("{")?;
                let mut body = vec![];
                let return_start = loop {
                    let at = p.token().span.start;
                    let word = p.ident()?;
                    if word == "return" {
                        break at;
                    }
                    if word != "let" {
                        return Err(p.error("expected let or return inside group"));
                    }
                    body.push(p.declaration(at)?);
                };
                let result = p.expr(0)?;
                p.expect(";")?;
                p.expect("}")?;
                doc.groups.push(Group {
                    name,
                    args,
                    ty,
                    body,
                    result,
                    return_start,
                    span: Span {
                        start,
                        end: p.tokens[p.index - 1].span.end,
                    },
                });
            }
            _ => {
                return Err(p.error("expected param, let, group, output, seed, viewbox, or editor"));
            }
        }
    }
    Ok(doc)
}

/// Explicit conservative formatter. Comments and strings are retained verbatim.
///
/// Normalizes CRLF outside strings to LF and ensures a final newline. It does
/// not regenerate source from the compiled graph or rearrange declarations.
///
/// # Errors
/// Returns syntax diagnostics for input that cannot be parsed.
pub fn format_source(source: &str) -> Result<String, Diagnostic> {
    parse(source)?;
    // Retain tokens and comments; normalize line endings outside strings only.
    let tokens = lex(source)?;
    let mut out = String::new();
    let mut cursor = 0;
    for token in tokens
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::String(_)))
    {
        out.push_str(&source[cursor..token.span.start].replace("\r\n", "\n"));
        out.push_str(&source[token.span.start..token.span.end]);
        cursor = token.span.end;
    }
    out.push_str(&source[cursor..].replace("\r\n", "\n"));
    if !out.ends_with('\n') {
        out.push('\n');
    }
    Ok(out)
}

/// Retain comment trivia when a source edit replaces a compound expression with a literal.
///
/// # Errors
/// Returns a diagnostic when the original expression cannot be tokenized.
pub fn preserve_expression_comments(
    original: &str,
    replacement: &str,
) -> Result<String, Diagnostic> {
    let tokens = lex(original)?;
    let mut cursor = 0;
    let mut comments = Vec::new();
    for token in tokens {
        let gap = original[cursor..token.span.start].trim();
        if !gap.is_empty() && !replacement.contains(gap) {
            comments.push(gap.to_string());
        }
        cursor = token.span.end;
    }
    if comments.is_empty() {
        Ok(replacement.into())
    } else {
        Ok(format!("{}\n{replacement}", comments.join("\n")))
    }
}
