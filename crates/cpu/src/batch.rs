//! Batched evaluation: every node runs across up to [`LANES`] samples at once.
//!
//! The per-sample interpreter in `evaluation` walks the whole graph once per
//! sample, paying its dispatch cost for every node of every sample. Here each
//! scheduled node is dispatched once per batch and its arithmetic loops over the
//! lanes, which the compiler vectorizes. Values are stored component-major with
//! only the components their type uses: scalars one, vectors two, colors four.
//!
//! Functions run with a lane mask: branches split it, `break`, `continue`, and
//! `return` remove lanes, and loops end when no lane remains. Arithmetic runs on
//! every lane (results of inactive lanes are ignored); calls, resampling, and
//! footprints run only for active lanes. Every operation matches the per-sample
//! interpreter exactly, so both produce identical pixels.
// Loops index several per-lane arrays by lane; iterators would obscure that.
#![allow(clippy::needless_range_loop)]
use crate::*;

/// Samples evaluated together.
pub(crate) const LANES: usize = 64;
const L: usize = LANES;
type Reg = [f32; 4 * L];
const ZERO: Reg = [0.; 4 * L];

/// Positions and sampling footprints of the samples in a batch.
#[derive(Clone)]
pub(crate) struct Lanes {
    pub x: [f32; L],
    pub y: [f32; L],
    pub fp: [f32; L],
    pub n: usize,
}
impl Lanes {
    pub(crate) fn new() -> Self {
        Self {
            x: [0.; L],
            y: [0.; L],
            fp: [0.; L],
            n: 0,
        }
    }
}

/// A reusable stack of registers; grows to its high-water mark and stays there.
#[derive(Default)]
pub(crate) struct Scratch {
    regs: Vec<Reg>,
    top: usize,
}
impl Scratch {
    fn alloc(&mut self, count: usize) -> usize {
        let base = self.top;
        self.top += count;
        if self.regs.len() < self.top {
            self.regs.resize(self.top, ZERO);
        }
        base
    }
}

// Loop and function exits collected while executing a function body.
struct Exits {
    broke: u64,
    continued: u64,
}

fn width(ty: Type) -> usize {
    match ty {
        Type::Vec2 => 2,
        Type::Color => 4,
        _ => 1,
    }
}
fn safe_div(a: f32, b: f32) -> f32 {
    if b.abs() > 1e-20 { a / b } else { 0. }
}
fn truth(v: bool) -> f32 {
    if v { 1. } else { 0. }
}
fn smooth(a: f32, b: f32, x: f32) -> f32 {
    let t = if (b - a).abs() < 1e-10 {
        if x >= b { 1. } else { 0. }
    } else {
        ((x - a) / (b - a)).clamp(0., 1.)
    };
    t * t * (3. - 2. * t)
}
fn lanes_of(mask: u64) -> impl Iterator<Item = usize> {
    let mut m = mask;
    std::iter::from_fn(move || {
        if m == 0 {
            return None;
        }
        let lane = m.trailing_zeros() as usize;
        m &= m - 1;
        Some(lane)
    })
}

/// Batched evaluator for one render; shared by every thread of it.
pub(crate) struct BatchEvaluator<'a> {
    pub program: &'a Program,
    pub parameters: &'a [Value],
    pub time: f32,
    pub seed: u32,
    pub plan: &'a ExecutionPlan,
    widths: Vec<usize>,
}

impl<'a> BatchEvaluator<'a> {
    pub(crate) fn new(
        program: &'a Program,
        parameters: &'a [Value],
        time: f32,
        seed: u32,
        plan: &'a ExecutionPlan,
    ) -> Self {
        Self {
            program,
            parameters,
            time,
            seed,
            plan,
            widths: program.nodes.iter().map(|n| width(n.ty)).collect(),
        }
    }

    pub(crate) fn wrap_point(&self, p: [f32; 2]) -> (f32, f32) {
        self.wrap(p[0], p[1])
    }

    fn wrap(&self, x: f32, y: f32) -> (f32, f32) {
        if !self.program.tile {
            return (x, y);
        }
        let v = self.program.viewbox;
        let w = |q: f32, origin: f32, size: f32| {
            let d = q - origin;
            origin + d - size * (d / size).floor()
        };
        (w(x, v[0], v[2]), w(y, v[1], v[3]))
    }

    /// Evaluate an output root for a batch; returns its color in `[lane][component]`.
    pub(crate) fn eval_root(
        &self,
        root: usize,
        lanes: &Lanes,
        mask: u64,
        s: &mut Scratch,
        out: &mut [[f32; 4]],
    ) {
        let dest = s.alloc(1);
        self.eval_into(root, lanes, mask, 0, s, dest);
        let r = &s.regs[dest];
        let w = self.widths[root];
        for (lane, pixel) in out.iter_mut().enumerate().take(lanes.n) {
            for (c, value) in pixel.iter_mut().enumerate() {
                *value = r[c.min(w - 1) * L + lane];
            }
        }
        s.top = dest;
    }

    /// Evaluate a fused block for a batch; `take(result index, lane, value)`.
    pub(crate) fn eval_fused(
        &self,
        block: &FusedBlock,
        lanes: &Lanes,
        mask: u64,
        s: &mut Scratch,
        mut take: impl FnMut(usize, usize, [f32; 4]),
    ) {
        let base = s.alloc(block.nodes.len());
        self.run(&block.nodes, base, lanes, mask, 0, s);
        for (index, register) in block.results.iter().enumerate() {
            let node = block.nodes[*register].node;
            let w = self.widths[node];
            let r = &s.regs[base + register];
            for lane in 0..lanes.n {
                take(
                    index,
                    lane,
                    std::array::from_fn(|c| r[c.min(w - 1) * L + lane]),
                );
            }
        }
        s.top = base;
    }

    // Evaluate a scheduled expression root into register `dest`, which lies below
    // the scratch top. `frame` is the current function frame's base register.
    fn eval_into(
        &self,
        root: usize,
        lanes: &Lanes,
        mask: u64,
        frame: usize,
        s: &mut Scratch,
        dest: usize,
    ) {
        let block = self.plan.block(root).expect("scheduled expression root");
        let base = s.alloc(block.nodes.len());
        self.run(&block.nodes, base, lanes, mask, frame, s);
        let last = base + block.nodes.len() - 1;
        let value = s.regs[last];
        s.regs[dest] = value;
        s.top = base;
    }

    fn run(
        &self,
        nodes: &[ScheduledNode],
        base: usize,
        lanes: &Lanes,
        mask: u64,
        frame: usize,
        s: &mut Scratch,
    ) {
        // Functions take at most 16 arguments; operations at most four.
        let mut inputs = [0usize; 16];
        for (i, scheduled) in nodes.iter().enumerate() {
            for (slot, input) in scheduled.inputs.iter().enumerate() {
                // Lazy sources are evaluated by their sampling operation.
                inputs[slot] = input.map_or(usize::MAX, |r| base + r);
            }
            self.node(
                scheduled.node,
                &inputs[..scheduled.inputs.len()],
                base + i,
                lanes,
                mask,
                frame,
                s,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn node(
        &self,
        id: usize,
        inputs: &[usize],
        out: usize,
        lanes: &Lanes,
        mask: u64,
        frame: usize,
        s: &mut Scratch,
    ) {
        let node = &self.program.nodes[id];
        let n = lanes.n;
        let w = self.widths[id];
        // Operations that evaluate other expressions need the whole scratch stack.
        match &node.kind {
            NodeKind::Call(index) => {
                self.call(*index, inputs, out, lanes, mask, s);
                return;
            }
            NodeKind::Builtin(Op::SampleAt) => {
                self.sample_at(node, inputs[1], out, lanes, mask, frame, s);
                return;
            }
            NodeKind::Builtin(Op::Footprint) => {
                self.footprint_op(node, out, lanes, mask, frame, s);
                return;
            }
            _ => (),
        }
        let (head, tail) = s.regs.split_at_mut(out);
        let o = &mut tail[0];
        let input = |k: usize| &head[inputs[k]];
        let iw = |k: usize| self.widths[node.inputs[k]];
        // Component `c` of input `k`, broadcasting scalars.
        macro_rules! comp {
            ($k:expr, $c:expr) => {{
                let r = input($k);
                let c = if iw($k) == 1 { 0 } else { $c };
                &r[c * L..c * L + L]
            }};
        }
        macro_rules! map1 {
            ($f:expr) => {{
                for c in 0..w {
                    let a = comp!(0, c);
                    for lane in 0..n {
                        o[c * L + lane] = $f(a[lane]);
                    }
                }
            }};
        }
        macro_rules! map2 {
            ($f:expr) => {{
                for c in 0..w {
                    let a = comp!(0, c);
                    let b = comp!(1, c);
                    for lane in 0..n {
                        o[c * L + lane] = $f(a[lane], b[lane]);
                    }
                }
            }};
        }
        macro_rules! scalar {
            ($f:expr) => {{
                for lane in 0..n {
                    o[lane] = $f(lane);
                }
            }};
        }
        let a0 = |k: usize, lane: usize| head[inputs[k]][lane];
        match &node.kind {
            NodeKind::Constant(value) => {
                for c in 0..w {
                    o[c * L..c * L + n].fill(value.data[c]);
                }
            }
            NodeKind::Local(slot) => {
                let r = &head[frame + slot];
                for c in 0..w {
                    o[c * L..c * L + n].copy_from_slice(&r[c * L..c * L + n]);
                }
            }
            NodeKind::Position => {
                o[..n].copy_from_slice(&lanes.x[..n]);
                o[L..L + n].copy_from_slice(&lanes.y[..n]);
            }
            NodeKind::Time => o[..n].fill(self.time),
            NodeKind::Parameter(index) => {
                let data = self.parameters[*index].data;
                for c in 0..w {
                    o[c * L..c * L + n].fill(data[c]);
                }
            }
            NodeKind::Unary(op) => match op {
                UnaryOp::Neg => map1!(|v: f32| -v),
                UnaryOp::Not => scalar!(|lane| truth(a0(0, lane) == 0.)),
                UnaryOp::Plus => map1!(|v: f32| v),
            },
            NodeKind::Binary(op) => self.binary(*op, &mut map_ctx(o, w, n), |k, c| comp!(k, c)),
            NodeKind::Builtin(op) => {
                use Op::*;
                match op {
                    Add => map2!(|a: f32, b: f32| a + b),
                    Sub => map2!(|a: f32, b: f32| a - b),
                    Mul => map2!(|a: f32, b: f32| a * b),
                    Div => map2!(safe_div),
                    Mod => map2!(|a: f32, b: f32| a - b * safe_div(a, b).floor()),
                    Less => scalar!(|l| truth(a0(0, l) < a0(1, l))),
                    Greater => scalar!(|l| truth(a0(0, l) > a0(1, l))),
                    Equal => scalar!(|l| truth(a0(0, l) == a0(1, l))),
                    Not => scalar!(|l| truth(a0(0, l) == 0.)),
                    And => scalar!(|l| truth(a0(0, l) != 0. && a0(1, l) != 0.)),
                    Or => scalar!(|l| truth(a0(0, l) != 0. || a0(1, l) != 0.)),
                    ToInt => scalar!(|l| a0(0, l).clamp(-16_777_216., 16_777_216.).trunc()),
                    ToFloat | X => scalar!(|l| a0(0, l)),
                    Y => scalar!(|l| head[inputs[0]][L + l]),
                    Vec2 => {
                        for lane in 0..n {
                            o[lane] = a0(0, lane);
                            o[L + lane] = a0(1, lane);
                        }
                    }
                    Rgba => {
                        for lane in 0..n {
                            let d = a0(3, lane).clamp(0., 1.);
                            o[lane] = a0(0, lane) * d;
                            o[L + lane] = a0(1, lane) * d;
                            o[2 * L + lane] = a0(2, lane) * d;
                            o[3 * L + lane] = d;
                        }
                    }
                    Red | Green | Blue => {
                        let c = match op {
                            Red => 0,
                            Green => 1,
                            _ => 2,
                        };
                        let a = input(0);
                        scalar!(|l| safe_div(a[c * L + l], a[3 * L + l]))
                    }
                    Alpha => {
                        let a = input(0);
                        scalar!(|l| a[3 * L + l])
                    }
                    Sin => map1!(f32::sin),
                    Cos => map1!(f32::cos),
                    Tan => map1!(f32::tan),
                    Abs => map1!(f32::abs),
                    Floor => map1!(f32::floor),
                    Ceil => map1!(f32::ceil),
                    Fract => map1!(|v: f32| v - v.floor()),
                    Sqrt => map1!(|v: f32| v.max(0.).sqrt()),
                    Exp => map1!(|v: f32| v.min(80.).exp()),
                    Log => map1!(|v: f32| if v > 0. { v.ln() } else { 0. }),
                    Pow => {
                        for c in 0..w {
                            let a = comp!(0, c);
                            for lane in 0..n {
                                o[c * L + lane] = finite(a[lane].powf(a0(1, lane)));
                            }
                        }
                    }
                    Min => map2!(f32::min),
                    Max => map2!(f32::max),
                    Clamp => {
                        for c in 0..w {
                            let a = comp!(0, c);
                            for lane in 0..n {
                                let (lo, hi) = (a0(1, lane), a0(2, lane));
                                o[c * L + lane] = a[lane].clamp(lo.min(hi), lo.max(hi));
                            }
                        }
                    }
                    Step => scalar!(|l| truth(a0(1, l) >= a0(0, l))),
                    Smoothstep => scalar!(|l| smooth(a0(0, l), a0(1, l), a0(2, l))),
                    Mix => {
                        for c in 0..w {
                            let a = comp!(0, c);
                            let b = comp!(1, c);
                            for lane in 0..n {
                                o[c * L + lane] = a[lane] + (b[lane] - a[lane]) * a0(2, lane);
                            }
                        }
                    }
                    Select => {
                        for c in 0..w {
                            let b = comp!(1, c);
                            let e = comp!(2, c);
                            for lane in 0..n {
                                o[c * L + lane] = if a0(0, lane) != 0. { b[lane] } else { e[lane] };
                            }
                        }
                    }
                    Length => {
                        let a = input(0);
                        scalar!(|l: usize| -> f32 { (a[l] * a[l] + a[L + l] * a[L + l]).sqrt() })
                    }
                    Dot => {
                        let (a, b) = (input(0), input(1));
                        scalar!(|l| a[l] * b[l] + a[L + l] * b[L + l])
                    }
                    Normalize => {
                        let a = input(0);
                        for lane in 0..n {
                            let len = (a[lane] * a[lane] + a[L + lane] * a[L + lane]).sqrt();
                            o[lane] = safe_div(a[lane], len);
                            o[L + lane] = safe_div(a[L + lane], len);
                        }
                    }
                    Atan2 => scalar!(|l| a0(0, l).atan2(a0(1, l))),
                    Hash => {
                        let a = input(0);
                        scalar!(|l| {
                            let seed = (self.seed ^ (a0(1, l) as i32 as u32))
                                .wrapping_add(a0(2, l) as i32 as u32)
                                ^ (a0(3, l) as i32 as u32).wrapping_mul(0x68bc21eb);
                            hash2(
                                a[l].floor().clamp(-2147483648., 2147483520.) as i32,
                                a[L + l].floor().clamp(-2147483648., 2147483520.) as i32,
                                seed,
                            )
                        })
                    }
                    PixelSize => o[..n].copy_from_slice(&lanes.fp[..n]),
                    Over => {
                        let (a, b) = (input(0), input(1));
                        for lane in 0..n {
                            let aa = a[3 * L + lane].clamp(0., 1.);
                            for c in 0..3 {
                                o[c * L + lane] = a[c * L + lane] + b[c * L + lane] * (1. - aa);
                            }
                            o[3 * L + lane] = aa + b[3 * L + lane].clamp(0., 1.) * (1. - aa);
                        }
                    }
                    Mask | Opacity => {
                        for c in 0..w {
                            let a = comp!(0, c);
                            for lane in 0..n {
                                o[c * L + lane] = a[lane] * a0(1, lane).clamp(0., 1.);
                            }
                        }
                    }
                    SampleAt | Footprint => unreachable!("handled above"),
                }
            }
            NodeKind::Call(_) => unreachable!("handled above"),
        }
        // Like the per-sample interpreter, sanitize every computed value.
        if !matches!(node.kind, NodeKind::Builtin(Op::PixelSize)) {
            for c in 0..w {
                for v in &mut o[c * L..c * L + n] {
                    *v = finite(*v);
                }
            }
        }
    }

    fn binary<'r>(
        &self,
        op: BinaryOp,
        ctx: &mut MapCtx<'_>,
        comp: impl Fn(usize, usize) -> &'r [f32],
    ) {
        let (w, n) = (ctx.w, ctx.n);
        let o = &mut *ctx.o;
        let elementwise = |o: &mut Reg, f: &dyn Fn(f32, f32) -> f32| {
            for c in 0..w {
                let (a, b) = (comp(0, c), comp(1, c));
                for lane in 0..n {
                    o[c * L + lane] = f(a[lane], b[lane]);
                }
            }
        };
        let compare = |o: &mut Reg, f: &dyn Fn(f32, f32) -> bool| {
            let (a, b) = (comp(0, 0), comp(1, 0));
            for lane in 0..n {
                o[lane] = truth(f(a[lane], b[lane]));
            }
        };
        match op {
            BinaryOp::Add => {
                for c in 0..w {
                    let (a, b) = (comp(0, c), comp(1, c));
                    for lane in 0..n {
                        o[c * L + lane] = a[lane] + b[lane];
                    }
                }
            }
            BinaryOp::Sub => {
                for c in 0..w {
                    let (a, b) = (comp(0, c), comp(1, c));
                    for lane in 0..n {
                        o[c * L + lane] = a[lane] - b[lane];
                    }
                }
            }
            BinaryOp::Mul => {
                for c in 0..w {
                    let (a, b) = (comp(0, c), comp(1, c));
                    for lane in 0..n {
                        o[c * L + lane] = a[lane] * b[lane];
                    }
                }
            }
            BinaryOp::Div => elementwise(o, &safe_div),
            BinaryOp::Rem => elementwise(o, &|a, b| a - b * safe_div(a, b).floor()),
            BinaryOp::Less => compare(o, &|a, b| a < b),
            BinaryOp::Greater => compare(o, &|a, b| a > b),
            BinaryOp::LessEqual => compare(o, &|a, b| a <= b),
            BinaryOp::GreaterEqual => compare(o, &|a, b| a >= b),
            BinaryOp::Equal => compare(o, &|a, b| a == b),
            BinaryOp::NotEqual => compare(o, &|a, b| a != b),
            BinaryOp::And => compare(o, &|a, b| a != 0. && b != 0.),
            BinaryOp::Or => compare(o, &|a, b| a != 0. || b != 0.),
        }
    }

    // Call a function for the active lanes; its result lands in `out`.
    fn call(
        &self,
        index: usize,
        args: &[usize],
        out: usize,
        lanes: &Lanes,
        mask: u64,
        s: &mut Scratch,
    ) {
        let function = &self.program.functions[index];
        // Paths that never return yield zero, as in the per-sample interpreter.
        s.regs[out] = ZERO;
        if mask == 0 {
            return;
        }
        let frame = s.alloc(function.locals.len().max(1));
        for (slot, arg) in args.iter().enumerate() {
            let value = s.regs[*arg];
            s.regs[frame + slot] = value;
        }
        let mut exits = Exits {
            broke: 0,
            continued: 0,
        };
        self.execute(&function.body, mask, frame, out, lanes, &mut exits, s);
        s.top = frame;
    }

    // Execute statements for the lanes in `active`; returns the lanes that reach
    // the end of the list (the rest broke, continued, or returned).
    #[allow(clippy::too_many_arguments)]
    fn execute(
        &self,
        body: &[Instruction],
        active: u64,
        frame: usize,
        ret: usize,
        lanes: &Lanes,
        exits: &mut Exits,
        s: &mut Scratch,
    ) -> u64 {
        let mut active = active;
        for instruction in body {
            if active == 0 {
                return 0;
            }
            match instruction {
                Instruction::Store { slot, value } => {
                    let tmp = s.alloc(1);
                    self.eval_into(*value, lanes, active, frame, s, tmp);
                    let (head, tail) = s.regs.split_at_mut(tmp);
                    masked_copy(&mut head[frame + slot], &tail[0], active);
                    s.top = tmp;
                }
                Instruction::Return(value) => {
                    let tmp = s.alloc(1);
                    self.eval_into(*value, lanes, active, frame, s, tmp);
                    let (head, tail) = s.regs.split_at_mut(tmp);
                    masked_copy(&mut head[ret], &tail[0], active);
                    s.top = tmp;
                    return 0;
                }
                Instruction::Break => {
                    exits.broke |= active;
                    return 0;
                }
                Instruction::Continue => {
                    exits.continued |= active;
                    return 0;
                }
                Instruction::If { condition, yes, no } => {
                    let truthy = self.condition(*condition, lanes, active, frame, s);
                    let a = self.execute(yes, active & truthy, frame, ret, lanes, exits, s);
                    let b = self.execute(no, active & !truthy, frame, ret, lanes, exits, s);
                    active = a | b;
                }
                Instruction::For {
                    slot,
                    start,
                    end,
                    body,
                } => {
                    let outer = (exits.broke, exits.continued);
                    exits.broke = 0;
                    let mut live = active;
                    for index in *start..*end {
                        if live == 0 {
                            break;
                        }
                        let r = &mut s.regs[frame + slot];
                        for lane in lanes_of(live) {
                            for c in 0..4 {
                                r[c * L + lane] = index as f32;
                            }
                        }
                        exits.continued = 0;
                        let after = self.execute(body, live, frame, ret, lanes, exits, s);
                        live = after | exits.continued;
                    }
                    active = live | exits.broke;
                    (exits.broke, exits.continued) = outer;
                }
                Instruction::While {
                    condition,
                    limit,
                    body,
                } => {
                    let outer = (exits.broke, exits.continued);
                    exits.broke = 0;
                    let mut live = active;
                    let mut finished = 0;
                    for _ in 0..*limit {
                        if live == 0 {
                            break;
                        }
                        let truthy = self.condition(*condition, lanes, live, frame, s);
                        finished |= live & !truthy;
                        live &= truthy;
                        if live == 0 {
                            break;
                        }
                        exits.continued = 0;
                        let after = self.execute(body, live, frame, ret, lanes, exits, s);
                        live = after | exits.continued;
                    }
                    active = live | finished | exits.broke;
                    (exits.broke, exits.continued) = outer;
                }
            }
        }
        active
    }

    fn condition(
        &self,
        node: usize,
        lanes: &Lanes,
        active: u64,
        frame: usize,
        s: &mut Scratch,
    ) -> u64 {
        let tmp = s.alloc(1);
        self.eval_into(node, lanes, active, frame, s, tmp);
        let r = &s.regs[tmp];
        let mut truthy = 0;
        for lane in lanes_of(active) {
            if r[lane] != 0. {
                truthy |= 1 << lane;
            }
        }
        s.top = tmp;
        truthy
    }

    // Per-lane footprint of a vec2 coordinate expression: the larger forward
    // difference over one sample, evaluated without footprints.
    fn coordinate_footprint(
        &self,
        id: usize,
        lanes: &Lanes,
        mask: u64,
        frame: usize,
        s: &mut Scratch,
    ) -> [f32; L] {
        let mut result = [0.; L];
        let mut live = 0;
        for lane in lanes_of(mask) {
            if lanes.fp[lane] > 0. {
                live |= 1 << lane;
            }
        }
        if live == 0 {
            return result;
        }
        let regs = s.alloc(3);
        let mut shifted = lanes.clone();
        shifted.fp = [0.; L];
        self.eval_into(id, &shifted, live, frame, s, regs);
        for (k, (dx, dy)) in [(1., 0.), (0., 1.)].into_iter().enumerate() {
            for lane in 0..lanes.n {
                shifted.x[lane] = lanes.x[lane] + lanes.fp[lane] * dx;
                shifted.y[lane] = lanes.y[lane] + lanes.fp[lane] * dy;
            }
            self.eval_into(id, &shifted, live, frame, s, regs + 1 + k);
        }
        let (center, dx, dy) = (&s.regs[regs], &s.regs[regs + 1], &s.regs[regs + 2]);
        for lane in lanes_of(live) {
            let ex = (dx[lane] - center[lane]).hypot(dx[L + lane] - center[L + lane]);
            let ey = (dy[lane] - center[lane]).hypot(dy[L + lane] - center[L + lane]);
            result[lane] = ex.max(ey);
        }
        s.top = regs;
        result
    }

    #[allow(clippy::too_many_arguments)]
    fn sample_at(
        &self,
        node: &Node,
        coords: usize,
        out: usize,
        lanes: &Lanes,
        mask: u64,
        frame: usize,
        s: &mut Scratch,
    ) {
        let fp = self.coordinate_footprint(node.inputs[1], lanes, mask, frame, s);
        let mut moved = lanes.clone();
        let c = &s.regs[coords];
        for lane in 0..lanes.n {
            (moved.x[lane], moved.y[lane]) = self.wrap(c[lane], c[L + lane]);
            moved.fp[lane] = fp[lane];
        }
        self.eval_into(node.inputs[0], &moved, mask, frame, s, out);
    }

    fn footprint_op(
        &self,
        node: &Node,
        out: usize,
        lanes: &Lanes,
        mask: u64,
        frame: usize,
        s: &mut Scratch,
    ) {
        let src = node.inputs[0];
        let result = if self.program.nodes[src].ty == Type::Vec2 {
            self.coordinate_footprint(src, lanes, mask, frame, s)
        } else {
            // Scalar: |central difference in X| + |central difference in Y|.
            let mut result = [0.; L];
            let mut live = 0;
            for lane in lanes_of(mask) {
                if lanes.fp[lane] > 0. {
                    live |= 1 << lane;
                }
            }
            if live != 0 {
                let regs = s.alloc(4);
                let mut shifted = lanes.clone();
                shifted.fp = [0.; L];
                for (k, (dx, dy)) in [(0.5, 0.), (-0.5, 0.), (0., 0.5), (0., -0.5)]
                    .into_iter()
                    .enumerate()
                {
                    for lane in 0..lanes.n {
                        shifted.x[lane] = lanes.x[lane] + lanes.fp[lane] * dx;
                        shifted.y[lane] = lanes.y[lane] + lanes.fp[lane] * dy;
                    }
                    self.eval_into(src, &shifted, live, frame, s, regs + k);
                }
                for lane in lanes_of(live) {
                    let r = |k: usize| s.regs[regs + k][lane];
                    result[lane] = (r(0) - r(1)).abs() + (r(2) - r(3)).abs();
                }
                s.top = regs;
            }
            result
        };
        s.regs[out][..L].copy_from_slice(&result);
    }
}

// The output register and extents an elementwise operation writes.
struct MapCtx<'o> {
    o: &'o mut Reg,
    w: usize,
    n: usize,
}
fn map_ctx(o: &mut Reg, w: usize, n: usize) -> MapCtx<'_> {
    MapCtx { o, w, n }
}

fn masked_copy(dest: &mut Reg, src: &Reg, mask: u64) {
    for lane in lanes_of(mask) {
        for c in 0..4 {
            dest[c * L + lane] = src[c * L + lane];
        }
    }
}

/// Visit the sample points of one image row in batches of up to [`LANES`].
///
/// Samples go in pixel order, then `sy`, then `sx`, matching the per-sample
/// interpreter's accumulation order. `owners[lane]` is the pixel within the row.
/// When `clip` is set, samples outside the view box are left out of the mask
/// (they still get positions, and their results are ignored).
#[allow(clippy::too_many_arguments)]
pub(crate) fn row_batches(
    evaluator: &BatchEvaluator,
    mapping: &Mapping,
    x0: i64,
    y: i64,
    width: usize,
    samples: u32,
    clip: bool,
    cancellation: &Cancellation,
    mut visit: impl FnMut(&Lanes, u64, &[usize; L]) -> Result<(), String>,
) -> Result<(), String> {
    let count = (samples * samples) as usize;
    let total = width * count;
    let fp = mapping.units_per_pixel / samples as f32;
    let mut lanes = Lanes::new();
    let mut owners = [0usize; L];
    let mut start = 0;
    while start < total {
        if cancellation.is_cancelled() {
            return Err("render cancelled".into());
        }
        lanes.n = L.min(total - start);
        let mut mask = 0u64;
        for lane in 0..lanes.n {
            let i = start + lane;
            let (col, sub) = (i / count, i % count);
            let (sy, sx) = ((sub as u32) / samples, (sub as u32) % samples);
            let x = (x0 + col as i64) as f32 + (sx as f32 + 0.5) / samples as f32;
            let yy = y as f32 + (sy as f32 + 0.5) / samples as f32;
            let p = mapping.position(x, yy);
            if !clip || evaluator.program.tile || mapping.contains(p) {
                mask |= 1 << lane;
            }
            (lanes.x[lane], lanes.y[lane]) = evaluator.wrap_point(p);
            lanes.fp[lane] = fp;
            owners[lane] = col;
        }
        visit(&lanes, mask, &owners)?;
        start += lanes.n;
    }
    Ok(())
}
