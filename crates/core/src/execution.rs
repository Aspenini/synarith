//! Backend-independent expression scheduling. Loops remain structured instructions.
use crate::{Instruction, NodeKind, Op, Program};
use std::collections::BTreeSet;

/// Maximum total scheduled expression instructions per execution plan: 262144.
///
/// Bounds memory used when many roots share large subgraphs. Source graph and
/// per-sample work limits still apply independently.
pub const MAX_SCHEDULED_INSTRUCTIONS: usize = 262_144;

/// One typed graph operation with inputs remapped to expression-local registers.
#[derive(Clone, Debug)]
pub struct ScheduledNode {
    /// Original node index; preserves its type, operation, and source span.
    pub node: usize,
    /// Register indices in argument order. `None` retains a lazy source expression.
    pub inputs: Vec<Option<usize>>,
}

/// An expression evaluated once in dependency order within one sampling context.
///
/// The result occupies the last register. Registers never survive an instruction,
/// function mutation, coordinate change, or sampling-footprint change.
#[derive(Clone, Debug)]
pub struct ExpressionBlock {
    /// Graph node that produces the expression result.
    pub root: usize,
    /// Each entry writes the register at its index, reading only earlier registers.
    pub nodes: Vec<ScheduledNode>,
    /// Whether reuse avoids enough repeated work to justify CPU register dispatch.
    /// Small expression trees can be evaluated directly with less bookkeeping.
    pub reuse_registers: bool,
}

/// Reusable execution schedule shared by CPU evaluation and GPU code generation.
///
/// Common graph references are evaluated once per expression. Sampling sources
/// have independent blocks; their values are never reused at other coordinates.
/// Function bodies retain branches, bounded loops, and early exits. Only functions
/// reachable from selected outputs are scheduled, while semantic compilation still
/// validates all definitions. Keep this paired with its original immutable program.
#[derive(Clone, Debug)]
pub struct ExecutionPlan {
    blocks: Vec<Option<ExpressionBlock>>,
    /// Reachable function indices, in dependency order.
    pub functions: BTreeSet<usize>,
    /// Total operations stored across all expression blocks.
    pub instruction_count: usize,
}

fn roots(body: &[Instruction], queue: &mut Vec<usize>) {
    for instruction in body {
        match instruction {
            Instruction::Store { value, .. } | Instruction::Return(value) => queue.push(*value),
            Instruction::If { condition, yes, no } => {
                queue.push(*condition);
                roots(yes, queue);
                roots(no, queue);
            }
            Instruction::For { body, .. } => roots(body, queue),
            Instruction::While {
                condition, body, ..
            } => {
                queue.push(*condition);
                roots(body, queue);
            }
            _ => (),
        }
    }
}

impl ExecutionPlan {
    /// Schedule all outputs of a compiler-produced program for reuse across frames.
    ///
    /// # Errors
    /// Returns an error if scheduling exceeds its bounded instruction allocation.
    pub fn compile(program: &Program) -> Result<Self, String> {
        Self::build(program, program.outputs.values().copied().collect())
    }

    /// Schedule one output, excluding code reachable only from other outputs.
    ///
    /// # Errors
    /// Returns unknown-output or bounded scheduling-allocation errors.
    pub fn for_output(program: &Program, output: &str) -> Result<Self, String> {
        Self::build(program, vec![program.output(output)?])
    }

    fn build(program: &Program, mut queue: Vec<usize>) -> Result<Self, String> {
        let mut plan = Self {
            blocks: vec![None; program.nodes.len()],
            functions: BTreeSet::new(),
            instruction_count: 0,
        };
        while let Some(root) = queue.pop() {
            if plan.blocks[root].is_some() {
                continue;
            }
            let mut needed = BTreeSet::new();
            let mut pending = vec![root];
            while let Some(id) = pending.pop() {
                if !needed.insert(id) {
                    continue;
                }
                let node = &program.nodes[id];
                if let NodeKind::Call(index) = node.kind
                    && plan.functions.insert(index)
                {
                    roots(&program.functions[index].body, &mut queue);
                }
                let lazy = matches!(node.kind, NodeKind::Builtin(Op::SampleAt | Op::Footprint));
                if lazy {
                    queue.push(node.inputs[0]);
                    if matches!(node.kind, NodeKind::Builtin(Op::SampleAt)) {
                        // Coordinate expressions are also sampled for their footprint.
                        queue.push(node.inputs[1]);
                    }
                }
                pending.extend(
                    node.inputs
                        .iter()
                        .enumerate()
                        .filter_map(|(i, id)| (!(lazy && i == 0)).then_some(*id)),
                );
            }
            plan.instruction_count += needed.len();
            if plan.instruction_count > MAX_SCHEDULED_INSTRUCTIONS {
                return Err("execution plan exceeds 262144 scheduled instructions".into());
            }
            let mut registers = vec![None; program.nodes.len()];
            let mut nodes = Vec::with_capacity(needed.len());
            for id in needed {
                let node = &program.nodes[id];
                let inputs = node
                    .inputs
                    .iter()
                    .enumerate()
                    .map(|(i, input)| {
                        if i == 0
                            && matches!(node.kind, NodeKind::Builtin(Op::SampleAt | Op::Footprint))
                        {
                            None
                        } else {
                            Some(
                                registers[*input].expect("compiler dependencies precede consumers"),
                            )
                        }
                    })
                    .collect();
                registers[id] = Some(nodes.len());
                nodes.push(ScheduledNode { node: id, inputs });
            }
            // Cheap leaves are often repeated without repeating mathematical work.
            // Favor registers when a shared call, sampler, or substantial subgraph
            // would otherwise be recomputed; retain direct trees for small cases.
            let mut tree_cost: Vec<usize> = Vec::with_capacity(nodes.len());
            let mut scheduled_cost = 0usize;
            for n in &nodes {
                let own = match program.nodes[n.node].kind {
                    NodeKind::Constant(_)
                    | NodeKind::Local(_)
                    | NodeKind::Position
                    | NodeKind::Time
                    | NodeKind::Parameter(_) => 0,
                    NodeKind::Call(_) | NodeKind::Builtin(Op::SampleAt | Op::Footprint) => 64,
                    NodeKind::Builtin(
                        Op::Sin | Op::Cos | Op::Tan | Op::Pow | Op::Exp | Op::Log | Op::Hash,
                    ) => 8,
                    _ => 1,
                };
                scheduled_cost += own;
                tree_cost.push(
                    n.inputs
                        .iter()
                        .flatten()
                        .fold(own, |sum, i| sum.saturating_add(tree_cost[*i])),
                );
            }
            let recursive_cost = tree_cost.last().copied().unwrap_or(0);
            let reuse_registers = recursive_cost.saturating_sub(scheduled_cost) >= 32
                && recursive_cost > scheduled_cost.saturating_mul(2);
            plan.blocks[root] = Some(ExpressionBlock {
                root,
                nodes,
                reuse_registers,
            });
        }
        Ok(plan)
    }

    /// Return a scheduled expression; `None` means it is unreachable or not a root.
    pub fn block(&self, root: usize) -> Option<&ExpressionBlock> {
        self.blocks.get(root).and_then(Option::as_ref)
    }

    /// Iterate expression blocks in source graph order for backend compilation.
    pub fn blocks(&self) -> impl Iterator<Item = &ExpressionBlock> {
        self.blocks.iter().filter_map(Option::as_ref)
    }
}
