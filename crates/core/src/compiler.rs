use crate::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Maximum number of nodes in a compiler-produced graph (4096).
pub const MAX_NODES: usize = 4096;
/// Maximum permitted compiled expression depth (96).
pub const MAX_GRAPH_DEPTH: usize = 96;
/// Typed four-component runtime value.
///
/// Scalars are replicated, vectors use X/Y with zero Z/W, and colors are
/// linear premultiplied RGBA. Prefer the constructors over manually encoding values.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Value {
    /// The language type used to interpret the components.
    pub ty: Type,
    /// Splatted scalar, XY vector, or linear premultiplied RGBA components.
    pub data: [f32; 4],
}
impl Value {
    /// Construct a scalar float, replicating its value across all components.
    pub fn float(value: f32) -> Self {
        Self {
            ty: Type::Float,
            data: [value; 4],
        }
    }
    /// Construct a splatted integer value.
    ///
    /// Values outside ±16777216 may lose precision; [`Program::check_parameters`]
    /// rejects integer parameter values outside that exact representation range.
    pub fn int(value: i32) -> Self {
        Self {
            ty: Type::Int,
            data: [value as f32; 4],
        }
    }
    /// Construct a boolean using splatted zero or one.
    pub fn boolean(value: bool) -> Self {
        Self {
            ty: Type::Bool,
            data: [if value { 1. } else { 0. }; 4],
        }
    }
    /// Construct an XY vector with zero Z/W components.
    pub fn vec2(x: f32, y: f32) -> Self {
        Self {
            ty: Type::Vec2,
            data: [x, y, 0., 0.],
        }
    }
    /// Construct premultiplied color from **straight linear** RGB and alpha.
    ///
    /// Clamps alpha to `[0, 1]`; RGB is not clamped or decoded from sRGB.
    pub fn color(r: f32, g: f32, b: f32, a: f32) -> Self {
        let a = a.clamp(0., 1.);
        Self {
            ty: Type::Color,
            data: [r * a, g * a, b * a, a],
        }
    }
    /// Return whether every component is finite.
    pub fn finite(&self) -> bool {
        self.data.iter().all(|v| v.is_finite())
    }
}
/// The operation performed by a typed graph node.
#[derive(Clone, Debug)]
pub enum NodeKind {
    /// A typed constant encoded in runtime representation.
    Constant(Value),
    /// Document-space position `p`, supplied by the sampler.
    Position,
    /// Time `t` in seconds, replicated across components.
    Time,
    /// A render-time parameter index in declaration order.
    Parameter(usize),
    /// A compiler-validated unary operator (`+`, `-`, or `!`).
    Unary(String),
    /// A compiler-validated infix arithmetic, comparison, or logical operator.
    Binary(String),
    /// A registry operation with arguments in signature order.
    Builtin(Op),
}
/// One node in an acyclic compiled graph; inputs index earlier program nodes.
#[derive(Clone, Debug)]
pub struct Node {
    /// Operation, literal, or dynamic input evaluated at this node.
    pub kind: NodeKind,
    /// Ordered indices of argument nodes in [`Program::nodes`].
    pub inputs: Vec<usize>,
    /// Validated result type.
    pub ty: Type,
    /// Source expression that produced this node.
    pub span: Span,
    /// Whether the node depends on position, time, or a parameter.
    pub varying: bool,
}
/// A named render-time parameter and its validated constant default.
#[derive(Clone, Debug)]
pub struct ParameterInfo {
    /// Document parameter name.
    pub name: String,
    /// Declared type, retained by render-time overrides.
    pub ty: Type,
    /// Validated constant default in runtime representation.
    pub default: Value,
}
/// Reusable typed graph produced by [`compile`].
///
/// Graph fields support inspection. Renderers require compiler invariants;
/// manually introducing invalid indices, types, or cycles is unsupported.
#[derive(Clone, Debug)]
pub struct Program {
    /// Nodes in dependency order, with inputs preceding their consumers.
    pub nodes: Vec<Node>,
    /// Alphabetically ordered color output names mapped to root node indices.
    pub outputs: BTreeMap<String, usize>,
    /// Parameter definitions in declaration order.
    pub parameters: Vec<ParameterInfo>,
    /// Named compiled fields mapped to node indices, including expanded group locals.
    pub named_nodes: BTreeMap<String, usize>,
    /// Document-space `[x, y, width, height]` with positive extents.
    pub viewbox: [f32; 4],
    /// Exact default 32-bit seed, independent of f32 parameter encoding.
    pub seed: u32,
    /// Compiler estimate of expanded evaluation work per sample.
    pub estimated_cost: u64,
}
impl Program {
    /// Return parameter defaults in declaration order.
    pub fn defaults(&self) -> Vec<Value> {
        self.parameters.iter().map(|p| p.default).collect()
    }
    /// Find a declared parameter by name; returns `None` for unknown names.
    pub fn parameter_index(&self, name: &str) -> Option<usize> {
        self.parameters.iter().position(|p| p.name == name)
    }
    /// Resolve a named color output to its root node index.
    ///
    /// # Errors
    /// Returns an error for an unknown output name.
    pub fn output(&self, name: &str) -> Result<usize, String> {
        self.outputs
            .get(name)
            .copied()
            .ok_or_else(|| format!("unknown output '{name}'"))
    }
    /// Validate a complete parameter set in declaration order.
    ///
    /// # Errors
    /// Rejects count/type mismatches, nonfinite values, inexact or out-of-range
    /// integers, booleans other than zero/one, and alpha outside `[0, 1]`.
    pub fn check_parameters(&self, values: &[Value]) -> Result<(), String> {
        if values.len() != self.parameters.len() {
            return Err("incorrect parameter count".into());
        }
        for (p, v) in self.parameters.iter().zip(values) {
            if p.ty != v.ty || !v.finite() {
                return Err(format!(
                    "parameter '{}' requires a finite {} value",
                    p.name, p.ty
                ));
            }
            if v.ty == Type::Int && (v.data[0].fract() != 0. || v.data[0].abs() > 16_777_216.) {
                return Err(format!(
                    "parameter '{}' requires an exactly representable integer",
                    p.name
                ));
            }
            if v.ty == Type::Bool && v.data[0] != 0. && v.data[0] != 1. {
                return Err(format!("parameter '{}' requires true or false", p.name));
            }
            if v.ty == Type::Color && !(0.0..=1.0).contains(&v.data[3]) {
                return Err(format!("parameter '{}' requires alpha in [0,1]", p.name));
            }
        }
        Ok(())
    }
}

struct Compiler<'a> {
    doc: &'a Document,
    program: Program,
    resolved: BTreeMap<String, usize>,
    active: BTreeSet<String>,
    groups: Vec<String>,
    depth: usize,
}
impl<'a> Compiler<'a> {
    fn err(&self, span: Span, message: impl Into<String>) -> Diagnostic {
        Diagnostic::new(&self.doc.source, span, message)
    }
    fn push(
        &mut self,
        kind: NodeKind,
        inputs: Vec<usize>,
        ty: Type,
        span: Span,
    ) -> Result<usize, Diagnostic> {
        if self.program.nodes.len() >= MAX_NODES {
            return Err(self.err(span, "compiled graph exceeds 4096 nodes"));
        }
        let varying = matches!(
            kind,
            NodeKind::Position | NodeKind::Time | NodeKind::Parameter(_)
        ) || inputs.iter().any(|id| self.program.nodes[*id].varying);
        let id = self.program.nodes.len();
        self.program.nodes.push(Node {
            kind,
            inputs,
            ty,
            span,
            varying,
        });
        Ok(id)
    }
    fn literal(&mut self, value: Value, span: Span) -> Result<usize, Diagnostic> {
        self.push(NodeKind::Constant(value), vec![], value.ty, span)
    }
    fn resolve(&mut self, name: &str, span: Span) -> Result<usize, Diagnostic> {
        if let Some(id) = self.resolved.get(name) {
            return Ok(*id);
        }
        let Some(decl) = self
            .doc
            .declarations
            .iter()
            .find(|d| d.name == name)
            .cloned()
        else {
            return Err(self.err(span, format!("unknown name '{name}'")));
        };
        if !self.active.insert(name.into()) {
            return Err(self.err(span, format!("cyclic reference to '{name}'")));
        }
        let id = self.expr(&decl.expr, &BTreeMap::new())?;
        self.active.remove(name);
        self.resolved.insert(name.into(), id);
        self.program.named_nodes.insert(name.into(), id);
        Ok(id)
    }
    fn expr(&mut self, e: &Expr, locals: &BTreeMap<String, usize>) -> Result<usize, Diagnostic> {
        self.depth += 1;
        if self.depth > MAX_GRAPH_DEPTH {
            return Err(self.err(e.span, "graph nesting exceeds 96"));
        }
        let result = self.expr_inner(e, locals);
        self.depth -= 1;
        result
    }
    fn expr_inner(
        &mut self,
        e: &Expr,
        locals: &BTreeMap<String, usize>,
    ) -> Result<usize, Diagnostic> {
        let span = e.span;
        match &e.kind {
            ExprKind::Number(v, int) => {
                if *int && v.abs() > 16_777_216. {
                    return Err(self.err(span,"integer literals must be within +/-16777216; use a decimal for larger floats"));
                }
                self.literal(
                    Value {
                        ty: if *int { Type::Int } else { Type::Float },
                        data: [*v; 4],
                    },
                    span,
                )
            }
            ExprKind::Bool(b) => self.literal(Value::boolean(*b), span),
            ExprKind::String(_) => Err(self.err(
                span,
                "strings are only allowed in srgb() and editor metadata",
            )),
            ExprKind::Name(n) => {
                if let Some(id) = locals.get(n) {
                    return Ok(*id);
                }
                match n.as_str() {
                    "p" => self.push(NodeKind::Position, vec![], Type::Vec2, span),
                    "t" => self.push(NodeKind::Time, vec![], Type::Float, span),
                    "pi" => self.literal(Value::float(std::f32::consts::PI), span),
                    "tau" => self.literal(Value::float(std::f32::consts::TAU), span),
                    _ => self.resolve(n, span),
                }
            }
            ExprKind::Unary(op, a) => {
                let a = self.expr(a, locals)?;
                let ty = self.program.nodes[a].ty;
                if (op == "!" && ty != Type::Bool) || (op != "!" && ty == Type::Bool) {
                    return Err(self.err(span, "invalid unary operand type"));
                }
                self.push(NodeKind::Unary(op.clone()), vec![a], ty, span)
            }
            ExprKind::Binary(op, a, b) => {
                let a = self.expr(a, locals)?;
                let b = self.expr(b, locals)?;
                let ta = self.program.nodes[a].ty;
                let tb = self.program.nodes[b].ty;
                let ty = if matches!(op.as_str(), "&&" | "||") {
                    if ta != Type::Bool || tb != Type::Bool {
                        return Err(self.err(span, "logical operators require booleans"));
                    }
                    Type::Bool
                } else if matches!(op.as_str(), "<" | ">" | "<=" | ">=" | "==" | "!=") {
                    if !matches!(ta, Type::Float | Type::Int | Type::Bool)
                        || !matches!(tb, Type::Float | Type::Int | Type::Bool)
                        || ((ta == Type::Bool) != (tb == Type::Bool))
                    {
                        return Err(self.err(span, "comparisons require compatible scalars"));
                    }
                    Type::Bool
                } else {
                    if ta == Type::Bool || tb == Type::Bool {
                        return Err(self.err(span, "arithmetic requires numeric operands"));
                    }
                    if matches!(ta, Type::Float | Type::Int)
                        && matches!(tb, Type::Float | Type::Int)
                    {
                        if ta == Type::Float || tb == Type::Float || op == "/" {
                            Type::Float
                        } else {
                            Type::Int
                        }
                    } else if ta == tb {
                        ta
                    } else if matches!(ta, Type::Float | Type::Int) {
                        tb
                    } else if matches!(tb, Type::Float | Type::Int) {
                        ta
                    } else {
                        return Err(self.err(span, "incompatible arithmetic operands"));
                    }
                };
                if matches!(op.as_str(), "/" | "%") && self.constant_scalar(b) == Some(0.) {
                    return Err(self.err(span, "constant division by zero"));
                }
                self.push(NodeKind::Binary(op.clone()), vec![a, b], ty, span)
            }
            ExprKind::Call(name, args) => {
                if name == "srgb" {
                    if args.len() != 1 || args[0].name.is_some() {
                        return Err(self.err(span, "srgb expects one hexadecimal string"));
                    }
                    let ExprKind::String(s) = &args[0].value.kind else {
                        return Err(self.err(span, "srgb expects a hexadecimal string"));
                    };
                    return self.literal(parse_color(s).map_err(|m| self.err(span, m))?, span);
                }
                if let Some(group) = self.doc.groups.iter().find(|g| g.name == *name).cloned() {
                    if self.groups.contains(name) {
                        return Err(self.err(span, format!("recursive group '{name}'")));
                    }
                    let mut ids = vec![None; group.args.len()];
                    let mut positional = 0;
                    let mut named = false;
                    for arg in args {
                        let index = if let Some(name) = &arg.name {
                            named = true;
                            group
                                .args
                                .iter()
                                .position(|(n, _)| n == name)
                                .ok_or_else(|| {
                                    self.err(
                                        arg.value.span,
                                        format!("unknown group argument '{name}'"),
                                    )
                                })?
                        } else {
                            if named {
                                return Err(self.err(
                                    arg.value.span,
                                    "positional arguments must precede named arguments",
                                ));
                            }
                            let i = positional;
                            positional += 1;
                            i
                        };
                        if index >= ids.len() || ids[index].is_some() {
                            return Err(self.err(span, "extra or duplicate group argument"));
                        }
                        let id = self.expr(&arg.value, locals)?;
                        if !compatible(self.program.nodes[id].ty, group.args[index].1) {
                            return Err(self.err(arg.value.span, "incorrect group argument type"));
                        }
                        ids[index] = Some(id);
                    }
                    let ids: Vec<usize> = ids
                        .into_iter()
                        .collect::<Option<Vec<_>>>()
                        .ok_or_else(|| self.err(span, "missing group argument"))?;
                    let mut scope: BTreeMap<String, usize> =
                        group.args.iter().map(|(n, _)| n.clone()).zip(ids).collect();
                    self.groups.push(name.clone());
                    for (arg, id) in &scope {
                        self.program
                            .named_nodes
                            .insert(format!("{}::{arg}", group.name), *id);
                    }
                    let local_names: BTreeSet<_> =
                        group.body.iter().map(|d| d.name.as_str()).collect();
                    let mut pending: Vec<_> = group.body.iter().collect();
                    while !pending.is_empty() {
                        let index = pending
                            .iter()
                            .position(|decl| {
                                let mut refs = vec![];
                                expression_names(&decl.expr, &mut refs);
                                refs.iter().all(|n| {
                                    !local_names.contains(n.as_str()) || scope.contains_key(n)
                                })
                            })
                            .ok_or_else(|| self.err(group.span, "cyclic group-local references"))?;
                        let decl = pending.remove(index);
                        let id = self.expr(&decl.expr, &scope)?;
                        scope.insert(decl.name.clone(), id);
                        self.program
                            .named_nodes
                            .insert(format!("{}::{}", group.name, decl.name), id);
                    }
                    let id = self.expr(&group.result, &scope)?;
                    self.groups.pop();
                    if !compatible(self.program.nodes[id].ty, group.ty) {
                        return Err(
                            self.err(span, format!("group '{name}' must return {}", group.ty))
                        );
                    }
                    return if self.program.nodes[id].ty != group.ty {
                        self.push(NodeKind::Unary("+".into()), vec![id], group.ty, span)
                    } else {
                        Ok(id)
                    };
                }
                let Some(op) = operation(name) else {
                    return Err(self.err(span, format!("unknown operation '{name}'")));
                };
                let ids = self.arguments(args, &op.inputs, locals, span)?;
                if matches!(op.op, Op::Mix | Op::Min | Op::Max | Op::Select) {
                    let (a, b) = if op.op == Op::Select { (1, 2) } else { (0, 1) };
                    let ta = self.program.nodes[ids[a]].ty;
                    let tb = self.program.nodes[ids[b]].ty;
                    if ta != tb
                        && !matches!(
                            (ta, tb),
                            (Type::Int, Type::Float) | (Type::Float, Type::Int)
                        )
                    {
                        return Err(self.err(span, "inputs must have compatible types"));
                    }
                }
                for (index, input) in op.inputs.iter().enumerate() {
                    if let Some(value) = self.constant_scalar(ids[index]) {
                        let valid = value.is_finite()
                            && match input.name {
                                "octaves" => (1. ..=12.).contains(&value) && value.fract() == 0.,
                                "iterations" => {
                                    (1. ..=1024.).contains(&value) && value.fract() == 0.
                                }
                                "sides" => (3. ..=64.).contains(&value) && value.fract() == 0.,
                                "points" => (2. ..=32.).contains(&value) && value.fract() == 0.,
                                "radius" | "width" | "epsilon" | "duration" | "lacunarity"
                                | "tolerance" => value > 0.,
                                _ => true,
                            };
                        if !valid {
                            return Err(self.err(
                                span,
                                format!("invalid constant {} for {}", input.name, name),
                            ));
                        }
                    }
                }
                if op.op == Op::PeriodicNoise
                    && let Some(value) = self.constant_vector(ids[1])
                    && value.iter().any(|v| *v < 1. || v.fract() != 0.)
                {
                    return Err(self.err(span, "periodic_noise periods must be positive integers"));
                }
                for (index, input) in op.inputs.iter().enumerate() {
                    if matches!(input.name, "size" | "radii" | "period")
                        && let Some(v) = self.constant_vector(ids[index])
                        && v.iter().any(|v| *v <= 0.)
                    {
                        return Err(self.err(
                            span,
                            format!("{} must have positive components", input.name),
                        ));
                    }
                }
                if matches!(op.op, Op::Sqrt | Op::Log)
                    && let Some(v) = self.constant_scalar(ids[0])
                    && (v < 0. || (op.op == Op::Log && v == 0.))
                {
                    return Err(self.err(span, "invalid constant mathematical domain"));
                }
                let mut ty = match op.result {
                    ReturnType::Exact(ty) => ty,
                    ReturnType::Input(i) => self.program.nodes[ids[i]].ty,
                };
                if matches!(op.op, Op::Mix | Op::Min | Op::Max | Op::Select) {
                    let (a, b) = if op.op == Op::Select { (1, 2) } else { (0, 1) };
                    if matches!(self.program.nodes[ids[a]].ty, Type::Int | Type::Float)
                        && matches!(self.program.nodes[ids[b]].ty, Type::Int | Type::Float)
                        && (self.program.nodes[ids[a]].ty == Type::Float
                            || self.program.nodes[ids[b]].ty == Type::Float)
                    {
                        ty = Type::Float;
                    }
                }
                if matches!(op.op, Op::Add | Op::Sub | Op::Mul | Op::Div | Op::Mod) {
                    let ta = self.program.nodes[ids[0]].ty;
                    let tb = self.program.nodes[ids[1]].ty;
                    ty = if matches!(ta, Type::Float | Type::Int)
                        && matches!(tb, Type::Float | Type::Int)
                    {
                        if ta == Type::Float || tb == Type::Float || op.op == Op::Div {
                            Type::Float
                        } else {
                            Type::Int
                        }
                    } else if ta == tb {
                        ta
                    } else if matches!(ta, Type::Float | Type::Int) {
                        tb
                    } else if matches!(tb, Type::Float | Type::Int) {
                        ta
                    } else {
                        return Err(self.err(span, "incompatible arithmetic operands"));
                    };
                    if matches!(op.op, Op::Div | Op::Mod)
                        && self.constant_scalar(ids[1]) == Some(0.)
                    {
                        return Err(self.err(span, "constant division by zero"));
                    }
                }
                if ty == Type::Int
                    && matches!(
                        op.op,
                        Op::Sin
                            | Op::Cos
                            | Op::Tan
                            | Op::Sqrt
                            | Op::Exp
                            | Op::Log
                            | Op::Pow
                            | Op::Mix
                            | Op::Clamp
                    )
                {
                    ty = Type::Float;
                }
                self.push(NodeKind::Builtin(op.op), ids, ty, span)
            }
        }
    }
    fn constant_scalar(&self, id: usize) -> Option<f32> {
        let node = &self.program.nodes[id];
        if !matches!(node.ty, Type::Float | Type::Int | Type::Bool) {
            return None;
        }
        match &node.kind {
            NodeKind::Constant(v) => Some(v.data[0]),
            NodeKind::Unary(op) => self
                .constant_scalar(self.program.nodes[id].inputs[0])
                .map(|v| if op == "-" { -v } else { v }),
            NodeKind::Binary(op) => {
                let a = self.constant_scalar(node.inputs[0])?;
                let b = self.constant_scalar(node.inputs[1])?;
                match op.as_str() {
                    "+" => Some(a + b),
                    "-" => Some(a - b),
                    "*" => Some(a * b),
                    "/" => Some(a / b),
                    "%" => Some(a - b * (a / b).floor()),
                    _ => None,
                }
            }
            _ => None,
        }
    }
    fn constant_vector(&self, id: usize) -> Option<[f32; 2]> {
        let node = &self.program.nodes[id];
        match &node.kind {
            NodeKind::Constant(v) if v.ty == Type::Vec2 => Some([v.data[0], v.data[1]]),
            NodeKind::Builtin(Op::Vec2) => Some([
                self.constant_scalar(node.inputs[0])?,
                self.constant_scalar(node.inputs[1])?,
            ]),
            _ => None,
        }
    }
    fn arguments(
        &mut self,
        args: &[Argument],
        specs: &[Input],
        locals: &BTreeMap<String, usize>,
        span: Span,
    ) -> Result<Vec<usize>, Diagnostic> {
        let mut assigned = vec![None; specs.len()];
        let mut next = 0;
        let mut named = false;
        for arg in args {
            let index = if let Some(name) = &arg.name {
                named = true;
                specs
                    .iter()
                    .position(|s| s.name == name)
                    .ok_or_else(|| self.err(arg.value.span, format!("unknown argument '{name}'")))?
            } else {
                if named {
                    return Err(self.err(
                        arg.value.span,
                        "positional arguments must precede named arguments",
                    ));
                }
                let i = next;
                next += 1;
                i
            };
            if index >= specs.len() {
                return Err(self.err(span, "too many arguments"));
            }
            if assigned[index].is_some() {
                return Err(self.err(arg.value.span, "duplicate argument"));
            }
            let id = self.expr(&arg.value, locals)?;
            let actual = self.program.nodes[id].ty;
            let valid = match specs[index].ty {
                ArgType::Exact(ty) => compatible(actual, ty),
                ArgType::Numeric => actual != Type::Bool,
                ArgType::Any => true,
            };
            if !valid {
                return Err(self.err(
                    arg.value.span,
                    format!(
                        "incorrect type {} for argument '{}'",
                        actual, specs[index].name
                    ),
                ));
            }
            assigned[index] = Some(id);
        }
        let mut ids = vec![];
        for (index, spec) in specs.iter().enumerate() {
            ids.push(if let Some(id) = assigned[index] {
                id
            } else if let Some(value) = spec.default {
                self.literal(
                    Value {
                        ty: value.ty,
                        data: value.data,
                    },
                    span,
                )?
            } else {
                return Err(self.err(span, format!("missing argument '{}'", spec.name)));
            });
        }
        Ok(ids)
    }
}
/// Whether an actual type can be passed to an expected type. Integers promote to float.
pub fn compatible(a: Type, b: Type) -> bool {
    a == b || matches!((a, b), (Type::Int, Type::Float))
}
fn expression_names(expr: &Expr, names: &mut Vec<String>) {
    match &expr.kind {
        ExprKind::Name(n) => names.push(n.clone()),
        ExprKind::Unary(_, a) => expression_names(a, names),
        ExprKind::Binary(_, a, b) => {
            expression_names(a, names);
            expression_names(b, names)
        }
        ExprKind::Call(_, args) => {
            for arg in args {
                expression_names(&arg.value, names)
            }
        }
        _ => (),
    }
}
/// Decode one sRGB channel to linear light using the sRGB transfer function.
///
/// Does not clamp the input; normal encoded channels lie in `[0, 1]`.
pub fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}
/// Encode one linear channel to sRGB, treating negative input as zero.
///
/// Values above one remain above one; byte exporters perform the final clamp.
pub fn linear_to_srgb(v: f32) -> f32 {
    let v = v.max(0.);
    if v <= 0.0031308 {
        12.92 * v
    } else {
        1.055 * v.powf(1. / 2.4) - 0.055
    }
}
/// Parse `#RRGGBB` or `#RRGGBBAA` sRGB into linear premultiplied color.
///
/// # Errors
/// Rejects a missing `#`, unsupported length, or nonhexadecimal digits.
///
/// ```
/// let white = synarith_core::parse_color("#ffffff80")?;
/// assert!((white.data[3] - 128.0 / 255.0).abs() < 1e-6);
/// assert_eq!(white.data[0], white.data[3]);
/// # Ok::<(), String>(())
/// ```
pub fn parse_color(text: &str) -> Result<Value, String> {
    let s = text
        .strip_prefix('#')
        .ok_or("expected #RRGGBB or #RRGGBBAA")?;
    if !matches!(s.len(), 6 | 8) || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("expected #RRGGBB or #RRGGBBAA".into());
    }
    let byte = |i| u8::from_str_radix(&s[i..i + 2], 16).unwrap() as f32 / 255.;
    Ok(Value::color(
        srgb_to_linear(byte(0)),
        srgb_to_linear(byte(2)),
        srgb_to_linear(byte(4)),
        if s.len() == 8 { byte(6) } else { 1. },
    ))
}

/// Resolve and type-check a parsed document into a reusable acyclic graph.
///
/// Validates every group, including unused groups, and expands calls.
/// Parameter values remain dynamic.
///
/// # Errors
/// Reports source-position diagnostics for unknown/duplicate names, unsupported
/// operations, incompatible types, cycles/recursion, invalid known constant
/// arguments, missing color outputs, or graph depth/node/evaluation limits.
pub fn compile(doc: &Document) -> Result<Program, Diagnostic> {
    let mut names = BTreeSet::new();
    let reserved = |n: &str| matches!(n, "p" | "t" | "pi" | "tau" | "true" | "false");
    for (name, span) in doc
        .parameters
        .iter()
        .map(|d| (&d.name, d.span))
        .chain(doc.declarations.iter().map(|d| (&d.name, d.span)))
        .chain(doc.groups.iter().map(|g| (&g.name, g.span)))
    {
        if reserved(name) || !names.insert(name) {
            return Err(Diagnostic::new(
                &doc.source,
                span,
                format!("duplicate or reserved name '{name}'"),
            ));
        }
    }
    for group in &doc.groups {
        if operation(&group.name).is_some() || group.name == "srgb" {
            return Err(Diagnostic::new(
                &doc.source,
                group.span,
                "group name conflicts with a built-in operation",
            ));
        }
        let mut local = BTreeSet::new();
        for name in group
            .args
            .iter()
            .map(|(n, _)| n)
            .chain(group.body.iter().map(|d| &d.name))
        {
            if reserved(name) || !local.insert(name) {
                return Err(Diagnostic::new(
                    &doc.source,
                    group.span,
                    format!("duplicate or reserved group name '{name}'"),
                ));
            }
        }
    }
    let mut c = Compiler {
        doc,
        program: Program {
            nodes: vec![],
            outputs: BTreeMap::new(),
            parameters: vec![],
            named_nodes: BTreeMap::new(),
            viewbox: doc.viewbox,
            seed: doc.seed,
            estimated_cost: 0,
        },
        resolved: BTreeMap::new(),
        active: BTreeSet::new(),
        groups: vec![],
        depth: 0,
    };
    for param in &doc.parameters {
        let value =
            constant_expression(&param.default).map_err(|m| c.err(param.default.span, m))?;
        if !compatible(value.ty, param.ty) {
            return Err(c.err(
                param.span,
                format!("parameter '{}' requires {}", param.name, param.ty),
            ));
        }
        if param.ty == Type::Int && value.data[0].fract() != 0. {
            return Err(c.err(param.span, "integer parameter default must be integral"));
        }
        let index = c.program.parameters.len();
        c.program.parameters.push(ParameterInfo {
            name: param.name.clone(),
            ty: param.ty,
            default: Value {
                ty: param.ty,
                ..value
            },
        });
        let id = c.push(NodeKind::Parameter(index), vec![], param.ty, param.span)?;
        c.resolved.insert(param.name.clone(), id);
    }
    for decl in &doc.declarations {
        c.resolve(&decl.name, decl.span)?;
    }
    c.program
        .check_parameters(&c.program.defaults())
        .map_err(|m| c.err(Span::default(), m))?;
    // Validate unused groups too: a bad reusable definition must not hide behind an unused call.
    for group in &doc.groups {
        let args = group
            .args
            .iter()
            .map(|(_, ty)| Argument {
                name: None,
                value: placeholder(*ty, group.span),
            })
            .collect();
        c.expr(
            &Expr {
                span: group.span,
                kind: ExprKind::Call(group.name.clone(), args),
            },
            &BTreeMap::new(),
        )?;
    }
    for output in &doc.outputs {
        if c.program.outputs.contains_key(&output.name) {
            return Err(c.err(output.span, "duplicate output name"));
        }
        let id = c.expr(&output.expr, &BTreeMap::new())?;
        if c.program.nodes[id].ty != Type::Color {
            return Err(c.err(
                output.expr.span,
                "outputs must be colors; use rgba(), srgb(), fill(), or a color ramp",
            ));
        }
        c.program.outputs.insert(output.name.clone(), id);
    }
    if c.program.outputs.is_empty() {
        return Err(c.err(Span::default(), "document requires at least one output"));
    }
    // Cap expanded evaluation cost, including source re-sampling, before any renderer sees the graph.
    let mut costs = vec![];
    let mut depths = vec![];
    for node in &c.program.nodes {
        let base = node
            .inputs
            .iter()
            .map(|id| costs[*id])
            .sum::<u64>()
            .saturating_add(1);
        let multiplier = match node.kind {
            NodeKind::Builtin(Op::Mandelbrot) => 1024,
            NodeKind::Builtin(Op::Blur) => 9,
            NodeKind::Builtin(Op::Normal) => 5,
            NodeKind::Builtin(Op::Noise | Op::GradientNoise) => 4,
            NodeKind::Builtin(Op::Fill | Op::Stroke | Op::Outline | Op::Warp | Op::Transform) => 5,
            NodeKind::Builtin(Op::Fbm | Op::Turbulence | Op::Ridged) => 12,
            NodeKind::Builtin(Op::Bezier | Op::Polygon | Op::Star) => 64,
            NodeKind::Builtin(Op::Path) => 256,
            _ => 1,
        };
        let cost = base.saturating_mul(multiplier);
        costs.push(cost);
        let depth = node
            .inputs
            .iter()
            .map(|id| depths[*id])
            .max()
            .unwrap_or(0usize)
            + 1;
        depths.push(depth);
        if cost > 2_000_000 || depth > MAX_GRAPH_DEPTH {
            return Err(c.err(node.span, "graph exceeds bounded evaluation cost or depth"));
        }
    }
    c.program.estimated_cost = c
        .program
        .outputs
        .values()
        .map(|id| costs[*id])
        .max()
        .unwrap_or(0);
    Ok(c.program)
}

/// Evaluate the restricted literal/constructor grammar used by parameter defaults.
///
/// Supports numeric and boolean literals, applicable unary operators, and
/// constant `vec2`, `rgba`, and `srgb` constructors.
///
/// # Errors
/// Rejects dynamic references, unsupported expressions, incorrect arguments,
/// invalid colors, or integer literals outside the exact f32 integer range.
pub fn constant_expression(e: &Expr) -> Result<Value, String> {
    match &e.kind {
        ExprKind::Number(v, int) => {
            if *int && v.abs() > 16_777_216. {
                return Err("integer literal exceeds the exact integer range".into());
            }
            Ok(Value {
                ty: if *int { Type::Int } else { Type::Float },
                data: [*v; 4],
            })
        }
        ExprKind::Bool(b) => Ok(Value::boolean(*b)),
        ExprKind::Unary(op, a) => {
            let mut v = constant_expression(a)?;
            if op == "-" && v.ty != Type::Bool {
                for x in &mut v.data {
                    *x = -*x;
                }
            } else if op == "!" && v.ty == Type::Bool {
                v = Value::boolean(v.data[0] == 0.)
            } else if op != "+" || v.ty == Type::Bool {
                return Err("invalid constant expression".into());
            }
            Ok(v)
        }
        ExprKind::Call(name, args)
            if name == "srgb" && args.len() == 1 && args[0].name.is_none() =>
        {
            let ExprKind::String(s) = &args[0].value.kind else {
                return Err("srgb expects a string".into());
            };
            parse_color(s)
        }
        ExprKind::Call(name, args) if name == "vec2" => {
            let values = constant_arguments(args, &["x", "y"], &[None, Some(Value::float(0.))])?;
            let a = values[0];
            let b = values[1];
            if !matches!(a.ty, Type::Int | Type::Float) || !matches!(b.ty, Type::Int | Type::Float)
            {
                return Err("vec2 requires scalar constants".into());
            }
            Ok(Value::vec2(a.data[0], b.data[0]))
        }
        ExprKind::Call(name, args) if name == "rgba" => {
            let v = constant_arguments(
                args,
                &["r", "g", "b", "a"],
                &[None, None, None, Some(Value::float(1.))],
            )?;
            if v.iter().any(|x| !matches!(x.ty, Type::Float | Type::Int)) {
                return Err("rgba requires scalar constants".into());
            }
            Ok(Value::color(
                v[0].data[0],
                v[1].data[0],
                v[2].data[0],
                v.get(3).map_or(1., |x| x.data[0]),
            ))
        }
        _ => Err("parameter defaults must be literals, vec2(), rgba(), or srgb() constants".into()),
    }
}

fn constant_arguments(
    args: &[Argument],
    names: &[&str],
    defaults: &[Option<Value>],
) -> Result<Vec<Value>, String> {
    let mut values = vec![None; names.len()];
    let mut position = 0;
    let mut named = false;
    for arg in args {
        let index = if let Some(name) = &arg.name {
            named = true;
            names
                .iter()
                .position(|n| n == name)
                .ok_or_else(|| format!("unknown constant argument '{name}'"))?
        } else {
            if named {
                return Err("positional arguments must precede named arguments".into());
            }
            let i = position;
            position += 1;
            i
        };
        if index >= values.len() || values[index].is_some() {
            return Err("extra or duplicate constant argument".into());
        }
        values[index] = Some(constant_expression(&arg.value)?);
    }
    values
        .into_iter()
        .zip(defaults)
        .enumerate()
        .map(|(i, (v, d))| {
            v.or(*d)
                .ok_or_else(|| format!("missing constant argument '{}'", names[i]))
        })
        .collect()
}

fn placeholder(ty: Type, span: Span) -> Expr {
    let make = |kind| Expr { kind, span };
    let time = || make(ExprKind::Name("t".into()));
    let zero = || make(ExprKind::Number(0., false));
    let arg = |value| Argument { name: None, value };
    match ty {
        Type::Float => time(),
        Type::Int => make(ExprKind::Call("int".into(), vec![arg(time())])),
        Type::Bool => make(ExprKind::Binary(
            ">".into(),
            Box::new(time()),
            Box::new(zero()),
        )),
        Type::Vec2 => make(ExprKind::Name("p".into())),
        Type::Color => make(ExprKind::Call(
            "rgba".into(),
            vec![arg(time()), arg(zero()), arg(zero())],
        )),
    }
}
