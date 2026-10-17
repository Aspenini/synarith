use crate::Type;

/// Built-in operations implemented by both rendering backends.
///
/// Use [`operation_for`] for each operation's complete signature and defaults;
/// [`operations`] lists the language catalog, including discovery descriptions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    /// `mandelbrot(p, iterations: 128)` — smooth normalized escape count for `z² + p`.
    ///
    /// Starts at `z = 0` and escapes at radius 16. Returns one when no escape is
    /// observed, otherwise a smoothed value in `[0, 1)`. Iterations are bounded
    /// to 1–1024; finite iteration counts approximate membership. Supersampling
    /// improves boundary sampling; f32 arithmetic limits deep zoom precision.
    /// Tiny CPU/GPU rounding differences can amplify near the set boundary at
    /// high iteration counts, so those orbits need not match pixel-for-pixel.
    Mandelbrot,
    /// `add` — Numeric arithmetic with scalar broadcasting.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Add,
    /// `sub` — Numeric arithmetic with scalar broadcasting.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Sub,
    /// `mul` — Numeric arithmetic with scalar broadcasting.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Mul,
    /// `div` — Numeric arithmetic with scalar broadcasting.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Div,
    /// `mod` — Numeric arithmetic with scalar broadcasting.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Mod,
    /// `less` — Compare scalar values.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Less,
    /// `greater` — Compare scalar values.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Greater,
    /// `equal` — Compare scalar values.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Equal,
    /// `not` — Boolean negation.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Not,
    /// `and` — Boolean conjunction.
    ///
    /// See [`operation_for`] for argument types and defaults.
    And,
    /// `or` — Boolean disjunction.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Or,
    /// `int` — Truncate and clamp a scalar to the exact integer range.
    ///
    /// See [`operation_for`] for argument types and defaults.
    ToInt,
    /// `float` — Convert a scalar to float.
    ///
    /// See [`operation_for`] for argument types and defaults.
    ToFloat,
    /// `vec2` — A two-dimensional vector.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Vec2,
    /// `rgba` — Linear straight RGB and alpha, converted to premultiplied color.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Rgba,
    /// `x` — Vector X component.
    ///
    /// See [`operation_for`] for argument types and defaults.
    X,
    /// `y` — Vector Y component.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Y,
    /// `red` — Straight linear color channel.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Red,
    /// `green` — Straight linear color channel.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Green,
    /// `blue` — Straight linear color channel.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Blue,
    /// `alpha` — Straight linear color channel.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Alpha,
    /// `sin` — Component-wise numeric function; invalid domains yield zero.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Sin,
    /// `cos` — Component-wise numeric function; invalid domains yield zero.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Cos,
    /// `tan` — Component-wise numeric function; invalid domains yield zero.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Tan,
    /// `abs` — Component-wise numeric function; invalid domains yield zero.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Abs,
    /// `floor` — Component-wise numeric function; invalid domains yield zero.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Floor,
    /// `ceil` — Component-wise numeric function; invalid domains yield zero.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Ceil,
    /// `fract` — Component-wise numeric function; invalid domains yield zero.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Fract,
    /// `sqrt` — Component-wise numeric function; invalid domains yield zero.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Sqrt,
    /// `exp` — Component-wise numeric function; invalid domains yield zero.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Exp,
    /// `log` — Component-wise numeric function; invalid domains yield zero.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Log,
    /// `pow` — Component-wise power.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Pow,
    /// `min` — Component-wise minimum.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Min,
    /// `max` — Component-wise maximum.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Max,
    /// `clamp` — Clamp between scalar bounds.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Clamp,
    /// `step` — Zero below edge and one otherwise.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Step,
    /// `smoothstep` — Cubic interpolation between edges.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Smoothstep,
    /// `mix` — Linear interpolation of compatible values.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Mix,
    /// `select` — Choose a when condition is true, otherwise b.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Select,
    /// `length` — Vector length.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Length,
    /// `dot` — Dot product.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Dot,
    /// `normalize` — Unit vector; zero stays zero.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Normalize,
    /// `noise` — Seeded continuous two-dimensional noise.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Noise,
    /// `gradient_noise` — Seeded continuous two-dimensional noise.
    ///
    /// See [`operation_for`] for argument types and defaults.
    GradientNoise,
    /// `cellular` — Seeded continuous two-dimensional noise.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Cellular,
    /// `fbm` — Footprint-filtered fractal noise; 1 to 12 octaves.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Fbm,
    /// `turbulence` — Footprint-filtered fractal noise; 1 to 12 octaves.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Turbulence,
    /// `ridged` — Footprint-filtered fractal noise; 1 to 12 octaves.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Ridged,
    /// `periodic_noise` — Value noise with integral lattice periods.
    ///
    /// See [`operation_for`] for argument types and defaults.
    PeriodicNoise,
    /// `linear` — Projected gradient.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Linear,
    /// `radial` — Distance from center divided by radius.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Radial,
    /// `angular` — Normalized polar angle.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Angular,
    /// `ramp` — Four color stops at 0, p1, p2, and 1.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Ramp,
    /// `circle` — Circle signed distance, centered at the origin.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Circle,
    /// `box` — Rectangle signed distance; size is half extent.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Box,
    /// `rounded_box` — Rounded rectangle signed distance.
    ///
    /// See [`operation_for`] for argument types and defaults.
    RoundedBox,
    /// `ellipse` — Ellipse distance approximation.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Ellipse,
    /// `polygon` — Regular polygon signed distance.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Polygon,
    /// `star` — Star polygon signed distance.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Star,
    /// `bezier` — Cubic path stroke distance, evaluated with 64 continuous segments.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Bezier,
    /// `path` — Open or closed cubic path distance with document-unit tolerance (8 to 256 segments).
    ///
    /// See [`operation_for`] for argument types and defaults.
    Path,
    /// `union` — Union of signed distances.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Union,
    /// `intersect` — Intersection of signed distances.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Intersect,
    /// `subtract` — Remove shape b from shape a.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Subtract,
    /// `smooth_union` — Smooth signed-distance union.
    ///
    /// See [`operation_for`] for argument types and defaults.
    SmoothUnion,
    /// `fill` — Footprint-antialiased signed-distance fill.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Fill,
    /// `stroke` — Stroke around a signed-distance boundary.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Stroke,
    /// `outline` — Exterior outline of a signed distance.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Outline,
    /// `glow` — Exponential exterior glow in document units.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Glow,
    /// `translate` — Translate coordinates by offset.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Translate,
    /// `rotate` — Rotate coordinates in radians.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Rotate,
    /// `scale` — Scale coordinate axes.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Scale,
    /// `repeat` — Centered repetition with positive periods.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Repeat,
    /// `mirror` — Absolute mirrored coordinates.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Mirror,
    /// `polar` — Radius and angle in radians.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Polar,
    /// `warp` — Evaluate a source field at displaced coordinates.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Warp,
    /// `transform` — Evaluate a source field at arbitrary coordinates.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Transform,
    /// `over` — Premultiplied linear-light source-over blend.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Over,
    /// `multiply` — Premultiplied linear-light source-over blend.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Multiply,
    /// `screen` — Premultiplied linear-light source-over blend.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Screen,
    /// `overlay` — Premultiplied linear-light source-over blend.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Overlay,
    /// `mask` — Multiply premultiplied color by coverage.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Mask,
    /// `opacity` — Multiply premultiplied color by opacity.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Opacity,
    /// `invert` — Invert straight RGB, preserving alpha.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Invert,
    /// `grayscale` — Linear luminance, preserving alpha.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Grayscale,
    /// `contrast` — Contrast around linear middle gray.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Contrast,
    /// `saturate` — Luminance-based color saturation.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Saturate,
    /// `loop` — Periodic phase in [0, 1).
    ///
    /// See [`operation_for`] for argument types and defaults.
    Loop,
    /// `pulse` — Smooth periodic pulse in [0, 1].
    ///
    /// See [`operation_for`] for argument types and defaults.
    Pulse,
    /// `blur` — Bounded 3x3 Gaussian sampling in document units.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Blur,
    /// `normal` — Normal color from procedural height derivatives.
    ///
    /// See [`operation_for`] for argument types and defaults.
    Normal,
}
/// Type constraint applied to one operation input.
#[derive(Clone, Copy, Debug)]
pub enum ArgType {
    /// Requires this type, allowing integer-to-float promotion.
    Exact(Type),
    /// Accepts scalar, vector, or color numeric values.
    Numeric,
    /// Accepts any language value type.
    Any,
}
/// A typed constant used when an optional operation argument is omitted.
#[derive(Clone, Copy, Debug)]
pub struct DefaultValue {
    /// Type of the default constant.
    pub ty: Type,
    /// Runtime components; colors are linear premultiplied RGBA.
    pub data: [f32; 4],
}
/// One ordered operation argument, including its optional default.
#[derive(Clone, Copy, Debug)]
pub struct Input {
    /// Argument label accepted by named-call syntax.
    pub name: &'static str,
    /// Argument type constraint.
    pub ty: ArgType,
    /// Optional default; `None` means the argument is required.
    pub default: Option<DefaultValue>,
}
/// How a built-in operation derives its result type.
#[derive(Clone, Copy, Debug)]
pub enum ReturnType {
    /// Returns the specified type.
    Exact(Type),
    /// Derives its type from the input at this zero-based index.
    Input(usize),
}
/// Authoritative operation metadata shared by the compiler and discovery APIs.
#[derive(Clone, Debug)]
pub struct Operation {
    /// Backend operation identifier.
    pub op: Op,
    /// Function spelling accepted in `.syar` documents.
    pub name: &'static str,
    /// Human-readable operation family.
    pub category: &'static str,
    /// Description suitable for discovery tools.
    pub description: &'static str,
    /// Arguments in positional-call order.
    pub inputs: Vec<Input>,
    /// Result-type rule before numeric promotion.
    pub result: ReturnType,
}
fn f(name: &'static str) -> Input {
    Input {
        name,
        ty: ArgType::Exact(Type::Float),
        default: None,
    }
}
fn v(name: &'static str) -> Input {
    Input {
        name,
        ty: ArgType::Exact(Type::Vec2),
        default: None,
    }
}
fn c(name: &'static str) -> Input {
    Input {
        name,
        ty: ArgType::Exact(Type::Color),
        default: None,
    }
}
fn n(name: &'static str) -> Input {
    Input {
        name,
        ty: ArgType::Numeric,
        default: None,
    }
}
fn any(name: &'static str) -> Input {
    Input {
        name,
        ty: ArgType::Any,
        default: None,
    }
}
fn fd(name: &'static str, value: f32) -> Input {
    Input {
        default: Some(DefaultValue {
            ty: Type::Float,
            data: [value; 4],
        }),
        ..f(name)
    }
}
fn id(name: &'static str, value: i32) -> Input {
    Input {
        name,
        ty: ArgType::Exact(Type::Int),
        default: Some(DefaultValue {
            ty: Type::Int,
            data: [value as f32; 4],
        }),
    }
}
fn vd(name: &'static str, x: f32, y: f32) -> Input {
    Input {
        default: Some(DefaultValue {
            ty: Type::Vec2,
            data: [x, y, 0., 0.],
        }),
        ..v(name)
    }
}
fn cd(name: &'static str, data: [f32; 4]) -> Input {
    Input {
        default: Some(DefaultValue {
            ty: Type::Color,
            data,
        }),
        ..c(name)
    }
}

/// List the complete catalog of operations implemented by both backends.
///
/// ```
/// let noise = synarith_core::operation("noise").expect("registered operation");
/// assert_eq!(noise.op, synarith_core::Op::Noise);
/// assert!(noise.inputs.iter().any(|input| input.name == "seed"));
/// ```
pub fn operations() -> Vec<Operation> {
    use Op::*;
    use ReturnType::{Exact as R, Input as I};
    let mut ops = vec![];
    macro_rules! add { ($op:ident,$name:literal,$cat:literal,$doc:literal,[$($arg:expr),*],$ret:expr) => { ops.push(Operation { op: $op, name:$name, category:$cat, description:$doc, inputs:vec![$($arg),*], result:$ret }); }; }
    add!(
        Mandelbrot,
        "mandelbrot",
        "Fractals",
        "Smooth normalized escape count for z²+p; one means no escape within 1–1024 iterations.",
        [v("p"), id("iterations", 128)],
        R(Type::Float)
    );
    for (op, name) in [
        (Add, "add"),
        (Sub, "sub"),
        (Mul, "mul"),
        (Div, "div"),
        (Mod, "mod"),
    ] {
        ops.push(Operation {
            op,
            name,
            category: "Math",
            description: "Numeric arithmetic with scalar broadcasting.",
            inputs: vec![n("a"), n("b")],
            result: I(0),
        });
    }
    for (op, name) in [(Less, "less"), (Greater, "greater"), (Equal, "equal")] {
        ops.push(Operation {
            op,
            name,
            category: "Logic",
            description: "Compare scalar values.",
            inputs: vec![f("a"), f("b")],
            result: R(Type::Bool),
        });
    }
    let boolean = |name| Input {
        name,
        ty: ArgType::Exact(Type::Bool),
        default: None,
    };
    add!(
        Not,
        "not",
        "Logic",
        "Boolean negation.",
        [boolean("value")],
        R(Type::Bool)
    );
    add!(
        And,
        "and",
        "Logic",
        "Boolean conjunction.",
        [boolean("a"), boolean("b")],
        R(Type::Bool)
    );
    add!(
        Or,
        "or",
        "Logic",
        "Boolean disjunction.",
        [boolean("a"), boolean("b")],
        R(Type::Bool)
    );
    add!(
        ToInt,
        "int",
        "Math",
        "Truncate and clamp a scalar to the exact integer range.",
        [f("value")],
        R(Type::Int)
    );
    add!(
        ToFloat,
        "float",
        "Math",
        "Convert a scalar to float.",
        [f("value")],
        R(Type::Float)
    );
    add!(
        Vec2,
        "vec2",
        "Constructors",
        "A two-dimensional vector.",
        [f("x"), fd("y", 0.)],
        R(Type::Vec2)
    );
    add!(
        Rgba,
        "rgba",
        "Constructors",
        "Linear straight RGB and alpha, converted to premultiplied color.",
        [f("r"), f("g"), f("b"), fd("a", 1.)],
        R(Type::Color)
    );
    add!(
        X,
        "x",
        "Math",
        "Vector X component.",
        [v("value")],
        R(Type::Float)
    );
    add!(
        Y,
        "y",
        "Math",
        "Vector Y component.",
        [v("value")],
        R(Type::Float)
    );
    for (op, name) in [
        (Red, "red"),
        (Green, "green"),
        (Blue, "blue"),
        (Alpha, "alpha"),
    ] {
        ops.push(Operation {
            op,
            name,
            category: "Color",
            description: "Straight linear color channel.",
            inputs: vec![c("color")],
            result: R(Type::Float),
        });
    }
    for (op, name) in [
        (Sin, "sin"),
        (Cos, "cos"),
        (Tan, "tan"),
        (Abs, "abs"),
        (Floor, "floor"),
        (Ceil, "ceil"),
        (Fract, "fract"),
        (Sqrt, "sqrt"),
        (Exp, "exp"),
        (Log, "log"),
    ] {
        ops.push(Operation {
            op,
            name,
            category: "Math",
            description: "Component-wise numeric function; invalid domains yield zero.",
            inputs: vec![n("value")],
            result: I(0),
        });
    }
    add!(
        Pow,
        "pow",
        "Math",
        "Component-wise power.",
        [n("value"), f("exponent")],
        I(0)
    );
    add!(
        Min,
        "min",
        "Math",
        "Component-wise minimum.",
        [n("a"), n("b")],
        I(0)
    );
    add!(
        Max,
        "max",
        "Math",
        "Component-wise maximum.",
        [n("a"), n("b")],
        I(0)
    );
    add!(
        Clamp,
        "clamp",
        "Math",
        "Clamp between scalar bounds.",
        [n("value"), fd("low", 0.), fd("high", 1.)],
        I(0)
    );
    add!(
        Step,
        "step",
        "Math",
        "Zero below edge and one otherwise.",
        [f("edge"), f("value")],
        R(Type::Float)
    );
    add!(
        Smoothstep,
        "smoothstep",
        "Math",
        "Cubic interpolation between edges.",
        [f("low"), f("high"), f("value")],
        R(Type::Float)
    );
    add!(
        Mix,
        "mix",
        "Math",
        "Linear interpolation of compatible values.",
        [any("a"), any("b"), f("amount")],
        I(0)
    );
    add!(
        Select,
        "select",
        "Math",
        "Choose a when condition is true, otherwise b.",
        [
            Input {
                name: "condition",
                ty: ArgType::Exact(Type::Bool),
                default: None
            },
            any("a"),
            any("b")
        ],
        I(1)
    );
    add!(
        Length,
        "length",
        "Math",
        "Vector length.",
        [v("value")],
        R(Type::Float)
    );
    add!(
        Dot,
        "dot",
        "Math",
        "Dot product.",
        [v("a"), v("b")],
        R(Type::Float)
    );
    add!(
        Normalize,
        "normalize",
        "Math",
        "Unit vector; zero stays zero.",
        [v("value")],
        R(Type::Vec2)
    );
    for (op, name) in [
        (Noise, "noise"),
        (GradientNoise, "gradient_noise"),
        (Cellular, "cellular"),
    ] {
        ops.push(Operation {
            op,
            name,
            category: "Noise",
            description: "Seeded continuous two-dimensional noise.",
            inputs: vec![v("p"), id("seed", 0)],
            result: R(Type::Float),
        });
    }
    for (op, name) in [(Fbm, "fbm"), (Turbulence, "turbulence"), (Ridged, "ridged")] {
        ops.push(Operation {
            op,
            name,
            category: "Noise",
            description: "Footprint-filtered fractal noise; 1 to 12 octaves.",
            inputs: vec![
                v("p"),
                id("octaves", 6),
                fd("lacunarity", 2.),
                fd("gain", 0.5),
                id("seed", 0),
            ],
            result: R(Type::Float),
        });
    }
    add!(
        PeriodicNoise,
        "periodic_noise",
        "Noise",
        "Value noise with integral lattice periods.",
        [v("p"), vd("period", 4., 4.), id("seed", 0)],
        R(Type::Float)
    );
    add!(
        Linear,
        "linear",
        "Gradients",
        "Projected gradient.",
        [v("p"), vd("start", 0., 0.), vd("end", 1., 0.)],
        R(Type::Float)
    );
    add!(
        Radial,
        "radial",
        "Gradients",
        "Distance from center divided by radius.",
        [v("p"), vd("center", 0.5, 0.5), fd("radius", 0.5)],
        R(Type::Float)
    );
    add!(
        Angular,
        "angular",
        "Gradients",
        "Normalized polar angle.",
        [v("p"), vd("center", 0.5, 0.5)],
        R(Type::Float)
    );
    add!(
        Ramp,
        "ramp",
        "Gradients",
        "Four color stops at 0, p1, p2, and 1.",
        [
            f("value"),
            cd("a", [0., 0., 0., 1.]),
            cd("b", [0.25, 0.25, 0.25, 1.]),
            cd("c", [0.75, 0.75, 0.75, 1.]),
            cd("d", [1.; 4]),
            fd("p1", 0.33333334),
            fd("p2", 0.6666667)
        ],
        R(Type::Color)
    );
    add!(
        Circle,
        "circle",
        "Shapes",
        "Circle signed distance, centered at the origin.",
        [v("p"), fd("radius", 0.3)],
        R(Type::Float)
    );
    add!(
        Box,
        "box",
        "Shapes",
        "Rectangle signed distance; size is half extent.",
        [v("p"), vd("size", 0.3, 0.2)],
        R(Type::Float)
    );
    add!(
        RoundedBox,
        "rounded_box",
        "Shapes",
        "Rounded rectangle signed distance.",
        [v("p"), vd("size", 0.3, 0.2), fd("radius", 0.05)],
        R(Type::Float)
    );
    add!(
        Ellipse,
        "ellipse",
        "Shapes",
        "Ellipse distance approximation.",
        [v("p"), vd("radii", 0.3, 0.2)],
        R(Type::Float)
    );
    add!(
        Polygon,
        "polygon",
        "Shapes",
        "Regular polygon signed distance.",
        [v("p"), id("sides", 6), fd("radius", 0.3)],
        R(Type::Float)
    );
    add!(
        Star,
        "star",
        "Shapes",
        "Star polygon signed distance.",
        [v("p"), id("points", 5), fd("outer", 0.3), fd("inner", 0.15)],
        R(Type::Float)
    );
    add!(
        Bezier,
        "bezier",
        "Paths",
        "Cubic path stroke distance, evaluated with 64 continuous segments.",
        [
            v("p"),
            vd("a", 0., 0.),
            vd("b", 0.2, 1.),
            vd("c", 0.8, 0.),
            vd("d", 1., 1.),
            fd("width", 0.01)
        ],
        R(Type::Float)
    );
    add!(
        Path,
        "path",
        "Paths",
        "Open or closed cubic path distance with document-unit tolerance (8 to 256 segments).",
        [
            v("p"),
            vd("a", 0.1, 0.7),
            vd("b", 0.1, 0.1),
            vd("c", 0.9, 0.1),
            vd("d", 0.9, 0.7),
            Input {
                name: "closed",
                ty: ArgType::Exact(Type::Bool),
                default: Some(DefaultValue {
                    ty: Type::Bool,
                    data: [1.; 4]
                })
            },
            fd("tolerance", 0.0001)
        ],
        R(Type::Float)
    );
    add!(
        Union,
        "union",
        "Distance",
        "Union of signed distances.",
        [f("a"), f("b")],
        R(Type::Float)
    );
    add!(
        Intersect,
        "intersect",
        "Distance",
        "Intersection of signed distances.",
        [f("a"), f("b")],
        R(Type::Float)
    );
    add!(
        Subtract,
        "subtract",
        "Distance",
        "Remove shape b from shape a.",
        [f("a"), f("b")],
        R(Type::Float)
    );
    add!(
        SmoothUnion,
        "smooth_union",
        "Distance",
        "Smooth signed-distance union.",
        [f("a"), f("b"), fd("radius", 0.1)],
        R(Type::Float)
    );
    add!(
        Fill,
        "fill",
        "Distance",
        "Footprint-antialiased signed-distance fill.",
        [f("distance"), cd("color", [1.; 4])],
        R(Type::Color)
    );
    add!(
        Stroke,
        "stroke",
        "Distance",
        "Stroke around a signed-distance boundary.",
        [f("distance"), fd("width", 0.02), cd("color", [1.; 4])],
        R(Type::Color)
    );
    add!(
        Outline,
        "outline",
        "Distance",
        "Exterior outline of a signed distance.",
        [f("distance"), fd("width", 0.02), cd("color", [1.; 4])],
        R(Type::Color)
    );
    add!(
        Glow,
        "glow",
        "Distance",
        "Exponential exterior glow in document units.",
        [f("distance"), fd("radius", 0.1), cd("color", [1.; 4])],
        R(Type::Color)
    );
    add!(
        Translate,
        "translate",
        "Coordinates",
        "Translate coordinates by offset.",
        [v("p"), vd("offset", 0.1, 0.)],
        R(Type::Vec2)
    );
    add!(
        Rotate,
        "rotate",
        "Coordinates",
        "Rotate coordinates in radians.",
        [v("p"), fd("angle", 0.)],
        R(Type::Vec2)
    );
    add!(
        Scale,
        "scale",
        "Coordinates",
        "Scale coordinate axes.",
        [v("p"), vd("factor", 2., 2.)],
        R(Type::Vec2)
    );
    add!(
        Repeat,
        "repeat",
        "Coordinates",
        "Centered repetition with positive periods.",
        [v("p"), vd("period", 0.25, 0.25)],
        R(Type::Vec2)
    );
    add!(
        Mirror,
        "mirror",
        "Coordinates",
        "Absolute mirrored coordinates.",
        [v("p")],
        R(Type::Vec2)
    );
    add!(
        Polar,
        "polar",
        "Coordinates",
        "Radius and angle in radians.",
        [v("p")],
        R(Type::Vec2)
    );
    add!(
        Warp,
        "warp",
        "Sampling",
        "Evaluate a source field at displaced coordinates.",
        [any("source"), v("displacement")],
        I(0)
    );
    add!(
        Transform,
        "transform",
        "Sampling",
        "Evaluate a source field at arbitrary coordinates.",
        [any("source"), v("coordinates")],
        I(0)
    );
    for (op, name) in [
        (Over, "over"),
        (Multiply, "multiply"),
        (Screen, "screen"),
        (Overlay, "overlay"),
    ] {
        ops.push(Operation {
            op,
            name,
            category: "Compositing",
            description: "Premultiplied linear-light source-over blend.",
            inputs: vec![c("foreground"), c("background")],
            result: R(Type::Color),
        });
    }
    add!(
        Mask,
        "mask",
        "Compositing",
        "Multiply premultiplied color by coverage.",
        [c("color"), f("amount")],
        R(Type::Color)
    );
    add!(
        Opacity,
        "opacity",
        "Compositing",
        "Multiply premultiplied color by opacity.",
        [c("color"), f("amount")],
        R(Type::Color)
    );
    add!(
        Invert,
        "invert",
        "Color",
        "Invert straight RGB, preserving alpha.",
        [c("color")],
        R(Type::Color)
    );
    add!(
        Grayscale,
        "grayscale",
        "Color",
        "Linear luminance, preserving alpha.",
        [c("color")],
        R(Type::Color)
    );
    add!(
        Contrast,
        "contrast",
        "Color",
        "Contrast around linear middle gray.",
        [c("color"), fd("amount", 1.)],
        R(Type::Color)
    );
    add!(
        Saturate,
        "saturate",
        "Color",
        "Luminance-based color saturation.",
        [c("color"), fd("amount", 1.)],
        R(Type::Color)
    );
    add!(
        Loop,
        "loop",
        "Animation",
        "Periodic phase in [0, 1). ",
        [f("time"), fd("duration", 1.)],
        R(Type::Float)
    );
    add!(
        Pulse,
        "pulse",
        "Animation",
        "Smooth periodic pulse in [0, 1].",
        [f("phase")],
        R(Type::Float)
    );
    add!(
        Blur,
        "blur",
        "Sampling",
        "Bounded 3x3 Gaussian sampling in document units.",
        [c("source"), fd("radius", 0.01)],
        R(Type::Color)
    );
    add!(
        Normal,
        "normal",
        "Sampling",
        "Normal color from procedural height derivatives.",
        [f("height"), fd("strength", 1.), fd("epsilon", 0.001)],
        R(Type::Color)
    );
    ops
}
/// Find a language operation by function name; returns `None` if unknown.
pub fn operation(name: &str) -> Option<Operation> {
    operations().into_iter().find(|op| op.name == name)
}
/// Look up the catalog entry for an operation identifier.
///
/// Every [`Op`] has a corresponding entry; missing entries indicate a registry bug.
pub fn operation_for(op: Op) -> Operation {
    operations()
        .into_iter()
        .find(|entry| entry.op == op)
        .unwrap()
}
