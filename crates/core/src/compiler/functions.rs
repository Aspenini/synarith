use super::*;

/// Maximum number of sample-local slots in one function, including arguments: 128.
pub const MAX_FUNCTION_LOCALS: usize = 128;
/// Maximum compile-time bound on a loop's iteration count: 4096.
pub const MAX_LOOP_ITERATIONS: u32 = 4096;
/// Maximum number of mathematical function definitions in a document: 128.
pub const MAX_FUNCTIONS: usize = 128;
/// Maximum total structured instructions across all function bodies: 4096.
pub const MAX_FUNCTION_INSTRUCTIONS: usize = 4096;

/// A validated, pure mathematical function, compiled once without loop unrolling.
#[derive(Clone, Debug)]
pub struct Function {
    /// Document function name.
    pub name: String,
    /// Ordered argument names/types; arguments occupy the initial frame slots.
    pub parameters: Vec<(String, Type)>,
    /// Declared result type.
    pub result: Type,
    /// Type of every frame slot, including arguments and block-local bindings.
    pub locals: Vec<Type>,
    /// Structured, type-checked control flow.
    pub body: Vec<Instruction>,
    /// Definition source range for resource-limit diagnostics.
    pub span: Span,
}
/// Backend-independent structured function instructions referencing expression nodes.
#[derive(Clone, Debug)]
pub enum Instruction {
    /// Evaluate a node and store its value in a typed local slot.
    Store {
        /// Frame slot index.
        slot: usize,
        /// Expression node index.
        value: usize,
    },
    /// Evaluate only the selected branch.
    If {
        /// Boolean expression node.
        condition: usize,
        /// True branch.
        yes: Vec<Instruction>,
        /// False branch.
        no: Vec<Instruction>,
    },
    /// Iterate a half-open constant integer range, updating an immutable index slot.
    For {
        /// Loop index frame slot.
        slot: usize,
        /// Inclusive start.
        start: u32,
        /// Exclusive end.
        end: u32,
        /// Loop body.
        body: Vec<Instruction>,
    },
    /// Test the condition before each iteration, stopping after at most `limit` iterations.
    While {
        /// Boolean expression node.
        condition: usize,
        /// Compile-time iteration limit.
        limit: u32,
        /// Loop body.
        body: Vec<Instruction>,
    },
    /// Exit the nearest loop.
    Break,
    /// Skip the remainder of the nearest loop's current iteration.
    Continue,
    /// Immediately return the expression node's value.
    Return(usize),
}
#[derive(Clone)]
struct Binding {
    node: usize,
    slot: usize,
    mutable: bool,
    ty: Type,
    constant: Option<usize>,
}
type Scope = BTreeMap<String, Binding>;

impl Compiler<'_> {
    pub(super) fn compile_function(&mut self, name: &str, span: Span) -> Result<usize, Diagnostic> {
        if let Some(index) = self.functions.get(name) {
            return Ok(*index);
        }
        if !self.active_functions.insert(name.into()) {
            return Err(self.err(span, format!("recursive function '{name}'")));
        }
        let definition = self
            .doc
            .functions
            .iter()
            .find(|f| f.name == name)
            .unwrap()
            .clone();
        if definition.args.len() > 16 {
            return Err(self.err(definition.span, "functions accept at most 16 arguments"));
        }
        let mut scope = Scope::new();
        let mut function = Function {
            name: name.into(),
            parameters: definition.args.clone(),
            result: definition.ty,
            locals: vec![],
            body: vec![],
            span: definition.span,
        };
        for (arg, ty) in &definition.args {
            self.bind(&mut function, &mut scope, arg, *ty, false, definition.span)?;
        }
        function.body = self.statements(&definition.body, &mut scope, &mut function, 0)?;
        if !returns(&function.body) {
            return Err(self.err(
                definition.span,
                format!(
                    "function '{name}' must return {} on every path",
                    definition.ty
                ),
            ));
        }
        let index = self.program.functions.len();
        self.program.functions.push(function);
        self.functions.insert(name.into(), index);
        self.active_functions.remove(name);
        Ok(index)
    }
    pub(super) fn function_call(
        &mut self,
        name: &str,
        args: &[Argument],
        locals: &BTreeMap<String, usize>,
        span: Span,
    ) -> Result<usize, Diagnostic> {
        let index = self.compile_function(name, span)?;
        let definition = self
            .doc
            .functions
            .iter()
            .find(|f| f.name == name)
            .unwrap()
            .clone();
        let mut inputs = vec![None; definition.args.len()];
        let mut positional = 0;
        let mut named = false;
        for arg in args {
            let slot = if let Some(label) = &arg.name {
                named = true;
                definition
                    .args
                    .iter()
                    .position(|a| a.0 == *label)
                    .ok_or_else(|| {
                        self.err(
                            arg.value.span,
                            format!("unknown function argument '{label}'"),
                        )
                    })?
            } else {
                if named {
                    return Err(self.err(
                        arg.value.span,
                        "positional arguments must precede named arguments",
                    ));
                }
                let slot = positional;
                positional += 1;
                slot
            };
            if slot >= inputs.len() || inputs[slot].is_some() {
                return Err(self.err(arg.value.span, "extra or duplicate function argument"));
            }
            let id = self.expr(&arg.value, locals)?;
            if !compatible(self.program.nodes[id].ty, definition.args[slot].1) {
                return Err(self.err(arg.value.span, "incorrect function argument type"));
            }
            inputs[slot] = Some(id);
        }
        let mut resolved = vec![];
        for (index, input) in inputs.into_iter().enumerate() {
            resolved.push(if let Some(id) = input {
                id
            } else if let Some(expr) = &definition.defaults[index] {
                let value = constant_expression(expr).map_err(|m| self.err(expr.span, m))?;
                self.literal(
                    Value {
                        ty: definition.args[index].1,
                        ..value
                    },
                    span,
                )?
            } else {
                return Err(self.err(span, "missing function argument"));
            });
        }
        let inputs = resolved;
        self.push(NodeKind::Call(index), inputs, definition.ty, span)
    }
    fn bind(
        &mut self,
        function: &mut Function,
        scope: &mut Scope,
        name: &str,
        ty: Type,
        mutable: bool,
        span: Span,
    ) -> Result<Binding, Diagnostic> {
        if reserved(name) || scope.contains_key(name) {
            return Err(self.err(span, format!("duplicate or reserved local name '{name}'")));
        }
        if function.locals.len() >= MAX_FUNCTION_LOCALS {
            return Err(self.err(span, "function exceeds 128 local slots"));
        }
        let slot = function.locals.len();
        function.locals.push(ty);
        let binding = Binding {
            node: self.push(NodeKind::Local(slot), vec![], ty, span)?,
            slot,
            mutable,
            ty,
            constant: None,
        };
        scope.insert(name.into(), binding.clone());
        Ok(binding)
    }
    fn local_expr(&mut self, expr: &Expr, scope: &Scope) -> Result<usize, Diagnostic> {
        self.expr(
            expr,
            &scope
                .iter()
                .map(|(name, b)| (name.clone(), b.constant.unwrap_or(b.node)))
                .collect(),
        )
    }
    fn boolean(&mut self, expr: &Expr, scope: &Scope) -> Result<usize, Diagnostic> {
        let node = self.local_expr(expr, scope)?;
        if self.program.nodes[node].ty != Type::Bool {
            return Err(self.err(expr.span, "control-flow condition must be bool"));
        }
        Ok(node)
    }
    fn bound(&mut self, expr: &Expr, scope: &Scope) -> Result<u32, Diagnostic> {
        let constants = scope
            .iter()
            .map(|(name, binding)| (name.clone(), binding.constant.unwrap_or(binding.node)))
            .collect();
        let node = self.expr(expr, &constants)?;
        let value = self.constant_scalar(node).ok_or_else(|| {
            self.err(
                expr.span,
                "loop bounds must be compile-time integer expressions",
            )
        })?;
        if self.program.nodes[node].ty != Type::Int
            || !value.is_finite()
            || !(0. ..=MAX_LOOP_ITERATIONS as f32).contains(&value)
            || value.fract() != 0.
        {
            return Err(self.err(expr.span, "loop bounds must be integers within 0..4096"));
        }
        Ok(value as u32)
    }
    fn statements(
        &mut self,
        source: &[Statement],
        scope: &mut Scope,
        function: &mut Function,
        loop_depth: usize,
    ) -> Result<Vec<Instruction>, Diagnostic> {
        self.depth += 1;
        if self.depth > MAX_GRAPH_DEPTH {
            return Err(self.err(function.span, "function nesting exceeds 96"));
        }
        let result = self.statements_inner(source, scope, function, loop_depth);
        self.depth -= 1;
        result
    }
    fn statements_inner(
        &mut self,
        source: &[Statement],
        scope: &mut Scope,
        function: &mut Function,
        loop_depth: usize,
    ) -> Result<Vec<Instruction>, Diagnostic> {
        let mut instructions = vec![];
        for statement in source {
            let span = statement.span;
            self.instruction_count += 1;
            if self.instruction_count > MAX_FUNCTION_INSTRUCTIONS {
                return Err(self.err(span, "document exceeds 4096 function instructions"));
            }
            if returns(&instructions)
                || matches!(
                    instructions.last(),
                    Some(Instruction::Break | Instruction::Continue)
                )
            {
                return Err(self.err(span, "unreachable statement after control-flow exit"));
            }
            let instruction = match &statement.kind {
                StatementKind::Binding(name, ty, mutable, expr) => {
                    let value = self.local_expr(expr, scope)?;
                    let actual = self.program.nodes[value].ty;
                    let ty = ty.unwrap_or(actual);
                    if !compatible(actual, ty) {
                        return Err(self.err(expr.span, "incorrect local initializer type"));
                    }
                    let slot = self.bind(function, scope, name, ty, *mutable, span)?.slot;
                    if !mutable && ty == Type::Int && self.constant_scalar(value).is_some() {
                        scope.get_mut(name).unwrap().constant = Some(value);
                    }
                    Instruction::Store { slot, value }
                }
                StatementKind::Assign(name, expr) => {
                    let binding = scope
                        .get(name)
                        .ok_or_else(|| {
                            self.err(span, format!("assignment to unknown local '{name}'"))
                        })?
                        .clone();
                    if !binding.mutable {
                        return Err(
                            self.err(span, format!("'{name}' is immutable; declare it with var"))
                        );
                    }
                    let value = self.local_expr(expr, scope)?;
                    if !compatible(self.program.nodes[value].ty, binding.ty) {
                        return Err(self.err(expr.span, "assignment changes the variable's type"));
                    }
                    Instruction::Store {
                        slot: binding.slot,
                        value,
                    }
                }
                StatementKind::If(condition, yes, no) => Instruction::If {
                    condition: self.boolean(condition, scope)?,
                    yes: self.statements(yes, &mut scope.clone(), function, loop_depth)?,
                    no: self.statements(no, &mut scope.clone(), function, loop_depth)?,
                },
                StatementKind::For(name, begin, end, body) => {
                    let start = self.bound(begin, scope)?;
                    let end = self.bound(end, scope)?;
                    if end < start {
                        return Err(self.err(span, "for range end must not precede its start"));
                    }
                    let mut inner = scope.clone();
                    let slot = self
                        .bind(function, &mut inner, name, Type::Int, false, span)?
                        .slot;
                    Instruction::For {
                        slot,
                        start,
                        end,
                        body: self.statements(body, &mut inner, function, loop_depth + 1)?,
                    }
                }
                StatementKind::While(condition, limit, body) => {
                    let condition = self.boolean(condition, scope)?;
                    let limit = self.bound(limit, scope)?;
                    Instruction::While {
                        condition,
                        limit,
                        body: self.statements(
                            body,
                            &mut scope.clone(),
                            function,
                            loop_depth + 1,
                        )?,
                    }
                }
                StatementKind::Break | StatementKind::Continue => {
                    if loop_depth == 0 {
                        return Err(self.err(span, "break and continue require an enclosing loop"));
                    }
                    if matches!(statement.kind, StatementKind::Break) {
                        Instruction::Break
                    } else {
                        Instruction::Continue
                    }
                }
                StatementKind::Return(expr) => {
                    let value = self.local_expr(expr, scope)?;
                    if !compatible(self.program.nodes[value].ty, function.result) {
                        return Err(self.err(
                            expr.span,
                            format!("function must return {}", function.result),
                        ));
                    }
                    Instruction::Return(value)
                }
            };
            instructions.push(instruction);
        }
        Ok(instructions)
    }
}
fn returns(body: &[Instruction]) -> bool {
    body.iter().any(|instruction| match instruction {
        Instruction::Return(_) => true,
        Instruction::If { yes, no, .. } => returns(yes) && returns(no),
        _ => false,
    })
}
pub(super) fn metrics(body: &[Instruction], costs: &[u64], depths: &[usize]) -> (u64, usize) {
    let (mut cost, mut depth) = (0u64, 0usize);
    for instruction in body {
        let (work, nesting) = match instruction {
            Instruction::Store { value, .. } | Instruction::Return(value) => {
                (costs[*value].saturating_add(1), depths[*value])
            }
            Instruction::If { condition, yes, no } => {
                let a = metrics(yes, costs, depths);
                let b = metrics(no, costs, depths);
                (
                    costs[*condition]
                        .saturating_add(a.0)
                        .saturating_add(b.0)
                        .saturating_add(1),
                    depths[*condition].max(a.1).max(b.1) + 1,
                )
            }
            Instruction::For {
                start, end, body, ..
            } => {
                let b = metrics(body, costs, depths);
                (
                    b.0.saturating_add(2)
                        .saturating_mul((end - start) as u64)
                        .saturating_add(1),
                    b.1 + 1,
                )
            }
            Instruction::While {
                condition,
                limit,
                body,
            } => {
                let b = metrics(body, costs, depths);
                (
                    b.0.saturating_add(costs[*condition])
                        .saturating_add(2)
                        .saturating_mul(*limit as u64)
                        .saturating_add(1),
                    b.1.max(depths[*condition]) + 1,
                )
            }
            _ => (1, 0),
        };
        cost = cost.saturating_add(work);
        depth = depth.max(nesting);
    }
    (cost, depth)
}
