// One evaluator implementation is instantiated for ordinary and deep-camera arithmetic.
// Constants and parameters retain the format's f32 representation; intermediate
// coordinates, operations, function frames, and sampling use the selected precision.
macro_rules! evaluator {
    ($scalar:ty) => {
        type Pixel = [$scalar; 4];
        fn finite(v: $scalar) -> $scalar {
            if v.is_finite() { v } else { 0. }
        }
        fn splat(x: $scalar) -> Pixel {
            [x; 4]
        }
        fn len(v: Pixel) -> $scalar {
            (v[0] * v[0] + v[1] * v[1]).sqrt()
        }
        fn mix(a: Pixel, b: Pixel, t: $scalar) -> Pixel {
            std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
        }
        fn smooth(a: $scalar, b: $scalar, x: $scalar) -> $scalar {
            let t = if (b - a).abs() < 1e-10 {
                if x >= b { 1. } else { 0. }
            } else {
                ((x - a) / (b - a)).clamp(0., 1.)
            };
            t * t * (3. - 2. * t)
        }
        fn safe_div(a: $scalar, b: $scalar) -> $scalar {
            if b.abs() > 1e-20 { a / b } else { 0. }
        }

        /// Fixed wrapping 32-bit avalanche hash shared with the WGSL backend.
        pub fn hash(mut x: u32) -> u32 {
            x ^= x >> 16;
            x = x.wrapping_mul(0x7feb352d);
            x ^= x >> 15;
            x = x.wrapping_mul(0x846ca68b);
            x ^ (x >> 16)
        }
        /// Hash an integer lattice coordinate and seed to a value in `[0, 1)`.
        pub fn hash2(x: i32, y: i32, seed: u32) -> $scalar {
            ((hash(
                (x as u32).wrapping_mul(0x9e3779b9) ^ (y as u32).wrapping_mul(0x85ebca6b) ^ seed,
            ) >> 8) as $scalar)
                / 16_777_216.
        }
        /// Low-level recursive CPU field evaluator.
        ///
        /// Prefer [`sample`] for checked point sampling. Callers of this type must
        /// provide a compiler-produced graph and a matching validated parameter slice.
        pub struct Evaluator<'a> {
            /// Compiler-produced graph; graph indices and types must remain valid.
            pub program: &'a Program,
            /// Complete parameter values matching [`Program::parameters`].
            pub parameters: &'a [Value],
            /// Finite frame time in seconds.
            pub time: $scalar,
            /// Exact effective seed after applying any override.
            pub seed: u32,
        }
        enum Flow {
            Next,
            Break,
            Continue,
            Return(Pixel),
        }
        impl Evaluator<'_> {
            fn call_function(
                &self,
                index: usize,
                argument: impl Fn(usize) -> Pixel,
                p: [$scalar; 2],
                fp: $scalar,
                plan: Option<&ExecutionPlan>,
            ) -> Pixel {
                let function = &self.program.functions[index];
                let mut frame = [[0.; 4]; MAX_FUNCTION_LOCALS];
                for (slot, value) in frame.iter_mut().enumerate().take(function.parameters.len()) {
                    *value = argument(slot);
                }
                match self.execute(
                    &function.body,
                    p,
                    fp,
                    &mut frame[..function.locals.len()],
                    plan,
                ) {
                    Flow::Return(value) => value,
                    _ => [0.; 4], // Compiler validation guarantees a return on every path.
                }
            }
            fn execute(
                &self,
                body: &[Instruction],
                p: [$scalar; 2],
                fp: $scalar,
                locals: &mut [Pixel],
                plan: Option<&ExecutionPlan>,
            ) -> Flow {
                for instruction in body {
                    match instruction {
                        Instruction::Store { slot, value } => {
                            locals[*slot] = self.eval_with(*value, p, fp, locals, plan)
                        }
                        Instruction::Return(value) => {
                            return Flow::Return(self.eval_with(*value, p, fp, locals, plan));
                        }
                        Instruction::Break => return Flow::Break,
                        Instruction::Continue => return Flow::Continue,
                        Instruction::If { condition, yes, no } => {
                            let selected =
                                if self.eval_with(*condition, p, fp, locals, plan)[0] != 0. {
                                    yes
                                } else {
                                    no
                                };
                            let flow = self.execute(selected, p, fp, locals, plan);
                            if !matches!(flow, Flow::Next) {
                                return flow;
                            }
                        }
                        Instruction::For {
                            slot,
                            start,
                            end,
                            body,
                        } => {
                            for index in *start..*end {
                                locals[*slot] = [index as $scalar; 4];
                                match self.execute(body, p, fp, locals, plan) {
                                    Flow::Break => break,
                                    Flow::Return(value) => return Flow::Return(value),
                                    _ => (),
                                }
                            }
                        }
                        Instruction::While {
                            condition,
                            limit,
                            body,
                        } => {
                            for _ in 0..*limit {
                                if self.eval_with(*condition, p, fp, locals, plan)[0] == 0. {
                                    break;
                                }
                                match self.execute(body, p, fp, locals, plan) {
                                    Flow::Break => break,
                                    Flow::Return(value) => return Flow::Return(value),
                                    _ => (),
                                }
                            }
                        }
                    }
                }
                Flow::Next
            }
            fn footprint(
                &self,
                id: usize,
                p: [$scalar; 2],
                fp: $scalar,
                locals: &[Pixel],
                plan: Option<&ExecutionPlan>,
            ) -> $scalar {
                if fp <= 0. {
                    return 0.;
                }
                let center = self.eval_with(id, p, 0., locals, plan);
                let dx = self.eval_with(id, [p[0] + fp, p[1]], 0., locals, plan);
                let dy = self.eval_with(id, [p[0], p[1] + fp], 0., locals, plan);
                ((dx[0] - center[0]).hypot(dx[1] - center[1]))
                    .max((dy[0] - center[0]).hypot(dy[1] - center[1]))
            }
            /// Evaluate a graph node at a document-space position and sampling footprint.
            ///
            /// Callers must supply a valid node index, finite inputs, a nonnegative footprint,
            /// and a complete validated parameter slice.
            /// Function-internal nodes require a local frame and must not be passed here;
            /// evaluate an output or function-call node instead.
            ///
            /// # Panics
            /// Panics on invalid graph/parameter indices; use [`sample`] for checked access.
            pub fn eval(&self, id: usize, p: [$scalar; 2], fp: $scalar) -> Pixel {
                self.eval_with(id, p, fp, &[], None)
            }
            pub(crate) fn eval_with(
                &self,
                id: usize,
                p: [$scalar; 2],
                fp: $scalar,
                locals: &[Pixel],
                plan: Option<&ExecutionPlan>,
            ) -> Pixel {
                if let Some(plan) = plan {
                    let block = plan.block(id).expect("scheduled expression root");
                    if block.reuse_registers {
                        return self.eval_registered(block, p, fp, locals, plan);
                    }
                }
                self.eval_node::<false>(id, p, fp, locals, plan, None)
            }
            #[inline(never)]
            fn eval_registered(
                &self,
                block: &ExpressionBlock,
                p: [$scalar; 2],
                fp: $scalar,
                locals: &[Pixel],
                plan: &ExecutionPlan,
            ) -> Pixel {
                let mut small = [[0.; 4]; 32];
                let mut large;
                let values: &mut [Pixel] = if block.nodes.len() <= small.len() {
                    &mut small[..block.nodes.len()]
                } else {
                    large = vec![[0.; 4]; block.nodes.len()];
                    &mut large
                };
                for (register, instruction) in block.nodes.iter().enumerate() {
                    values[register] = self.eval_node::<true>(
                        instruction.node,
                        p,
                        fp,
                        locals,
                        Some(plan),
                        Some((values, &instruction.inputs)),
                    );
                }
                values[block.nodes.len() - 1]
            }
            fn eval_node<const REGISTERED: bool>(
                &self,
                id: usize,
                p: [$scalar; 2],
                fp: $scalar,
                locals: &[Pixel],
                plan: Option<&ExecutionPlan>,
                registers: Option<(&[Pixel], &[Option<usize>])>,
            ) -> Pixel {
                let node = &self.program.nodes[id];
                let at = |index: usize| {
                    if REGISTERED {
                        let (values, inputs) = registers.unwrap();
                        values[inputs[index]
                            .expect("lazy sources are evaluated only by sampling operations")]
                    } else {
                        self.eval_node::<false>(node.inputs[index], p, fp, locals, plan, None)
                    }
                };
                let result = match &node.kind {
                    NodeKind::Constant(value) => value.data.map(<$scalar>::from),
                    NodeKind::Local(slot) => locals[*slot],
                    NodeKind::Call(index) => self.call_function(*index, at, p, fp, plan),
                    NodeKind::Position => [p[0], p[1], 0., 0.],
                    NodeKind::Time => splat(self.time),
                    NodeKind::Parameter(index) => self.parameters[*index].data.map(<$scalar>::from),
                    NodeKind::Unary(op) => {
                        let a = at(0);
                        match op.as_str() {
                            "-" => a.map(|v| -v),
                            "!" => splat(if a[0] == 0. { 1. } else { 0. }),
                            _ => a,
                        }
                    }
                    NodeKind::Binary(op) => {
                        let a = at(0);
                        let b = at(1);
                        std::array::from_fn(|i| match op.as_str() {
                            "+" => a[i] + b[i],
                            "-" => a[i] - b[i],
                            "*" => a[i] * b[i],
                            "/" => safe_div(a[i], b[i]),
                            "%" => a[i] - b[i] * (safe_div(a[i], b[i])).floor(),
                            "<" => {
                                if a[0] < b[0] {
                                    1.
                                } else {
                                    0.
                                }
                            }
                            ">" => {
                                if a[0] > b[0] {
                                    1.
                                } else {
                                    0.
                                }
                            }
                            "<=" => {
                                if a[0] <= b[0] {
                                    1.
                                } else {
                                    0.
                                }
                            }
                            ">=" => {
                                if a[0] >= b[0] {
                                    1.
                                } else {
                                    0.
                                }
                            }
                            "==" => {
                                if a[0] == b[0] {
                                    1.
                                } else {
                                    0.
                                }
                            }
                            "!=" => {
                                if a[0] != b[0] {
                                    1.
                                } else {
                                    0.
                                }
                            }
                            "&&" => {
                                if a[0] != 0. && b[0] != 0. {
                                    1.
                                } else {
                                    0.
                                }
                            }
                            "||" if (a[0] != 0. || b[0] != 0.) => 1.,
                            _ => 0.,
                        })
                    }
                    NodeKind::Builtin(op) => {
                        // Source expressions stay lazy, including through expanded library groups.
                        if *op == Op::SampleAt {
                            let coords = at(1);
                            let transformed_fp =
                                self.footprint(node.inputs[1], p, fp, locals, plan);
                            return self.eval_with(
                                node.inputs[0],
                                [coords[0], coords[1]],
                                transformed_fp,
                                locals,
                                plan,
                            );
                        }
                        if *op == Op::PixelSize {
                            return splat(fp);
                        }
                        if *op == Op::Footprint {
                            let src = node.inputs[0];
                            if fp <= 0. {
                                return splat(0.);
                            }
                            if self.program.nodes[src].ty == Type::Vec2 {
                                return splat(self.footprint(src, p, fp, locals, plan));
                            }
                            let dx = self.eval_with(src, [p[0] + fp * 0.5, p[1]], 0., locals, plan)
                                [0]
                                - self.eval_with(src, [p[0] - fp * 0.5, p[1]], 0., locals, plan)[0];
                            let dy = self.eval_with(src, [p[0], p[1] + fp * 0.5], 0., locals, plan)
                                [0]
                                - self.eval_with(src, [p[0], p[1] - fp * 0.5], 0., locals, plan)[0];
                            return splat(dx.abs() + dy.abs());
                        }
                        let a = at(0);
                        let b = if node.inputs.len() > 1 {
                            at(1)
                        } else {
                            [0.; 4]
                        };
                        let c = if node.inputs.len() > 2 {
                            at(2)
                        } else {
                            [0.; 4]
                        };
                        use Op::*;
                        match op {
                            Add => std::array::from_fn(|i| a[i] + b[i]),
                            Sub => std::array::from_fn(|i| a[i] - b[i]),
                            Mul => std::array::from_fn(|i| a[i] * b[i]),
                            Div => std::array::from_fn(|i| safe_div(a[i], b[i])),
                            Mod => {
                                std::array::from_fn(|i| a[i] - b[i] * safe_div(a[i], b[i]).floor())
                            }
                            Less => splat(if a[0] < b[0] { 1. } else { 0. }),
                            Greater => splat(if a[0] > b[0] { 1. } else { 0. }),
                            Equal => splat(if a[0] == b[0] { 1. } else { 0. }),
                            Not => splat(if a[0] == 0. { 1. } else { 0. }),
                            And => splat(if a[0] != 0. && b[0] != 0. { 1. } else { 0. }),
                            Or => splat(if a[0] != 0. || b[0] != 0. { 1. } else { 0. }),
                            ToInt => splat(a[0].clamp(-16_777_216., 16_777_216.).trunc()),
                            ToFloat => splat(a[0]),
                            Vec2 => [a[0], b[0], 0., 0.],
                            Rgba => {
                                let d = at(3)[0].clamp(0., 1.);
                                [a[0] * d, b[0] * d, c[0] * d, d]
                            }
                            X => splat(a[0]),
                            Y => splat(a[1]),
                            Red | Green | Blue => splat(safe_div(
                                a[match op {
                                    Red => 0,
                                    Green => 1,
                                    _ => 2,
                                }],
                                a[3],
                            )),
                            Alpha => splat(a[3]),
                            Sin => a.map(<$scalar>::sin),
                            Cos => a.map(<$scalar>::cos),
                            Tan => a.map(<$scalar>::tan),
                            Abs => a.map(<$scalar>::abs),
                            Floor => a.map(<$scalar>::floor),
                            Ceil => a.map(<$scalar>::ceil),
                            Fract => a.map(|v| v - v.floor()),
                            Sqrt => a.map(|v| v.max(0.).sqrt()),
                            Exp => a.map(|v| v.min(80.).exp()),
                            Log => a.map(|v| if v > 0. { v.ln() } else { 0. }),
                            Pow => a.map(|v| finite(v.powf(b[0]))),
                            Min => std::array::from_fn(|i| a[i].min(b[i])),
                            Max => std::array::from_fn(|i| a[i].max(b[i])),
                            Clamp => a.map(|v| v.clamp(b[0].min(c[0]), b[0].max(c[0]))),
                            Step => splat(if b[0] >= a[0] { 1. } else { 0. }),
                            Smoothstep => splat(smooth(a[0], b[0], c[0])),
                            Mix => mix(a, b, c[0]),
                            Select => {
                                if a[0] != 0. {
                                    b
                                } else {
                                    c
                                }
                            }
                            Length => splat(len(a)),
                            Dot => splat(a[0] * b[0] + a[1] * b[1]),
                            Normalize => {
                                let l = len(a);
                                [safe_div(a[0], l), safe_div(a[1], l), 0., 0.]
                            }
                            Atan2 => splat(a[0].atan2(b[0])),
                            Hash => {
                                let seed = (self.seed ^ (b[0] as i32 as u32))
                                    .wrapping_add(c[0] as i32 as u32)
                                    ^ (at(3)[0] as i32 as u32).wrapping_mul(0x68bc21eb);
                                splat(hash2(
                                    a[0].floor().clamp(-2147483648., 2147483520.) as i32,
                                    a[1].floor().clamp(-2147483648., 2147483520.) as i32,
                                    seed,
                                ))
                            }
                            Over => {
                                let aa = a[3].clamp(0., 1.);
                                [
                                    a[0] + b[0] * (1. - aa),
                                    a[1] + b[1] * (1. - aa),
                                    a[2] + b[2] * (1. - aa),
                                    aa + b[3].clamp(0., 1.) * (1. - aa),
                                ]
                            }
                            Mask | Opacity => a.map(|v| v * b[0].clamp(0., 1.)),
                            SampleAt | Footprint | PixelSize => unreachable!(),
                        }
                    }
                };
                result.map(finite)
            }
        }
    };
}
pub mod single {
    use super::super::*;
    evaluator!(f32);
}
pub mod double {
    use super::super::*;
    evaluator!(f64);
}
