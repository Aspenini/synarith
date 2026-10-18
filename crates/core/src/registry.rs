use crate::Type;

/// Built-in operations implemented by both rendering backends.
///
/// Use [`operation_for`] for each operation's complete signature and defaults;
/// [`operations`] lists the language catalog, including discovery descriptions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
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
    /// `atan2(y, x)` — quadrant-aware arctangent in radians.
    Atan2,
    /// `hash(cell, seed, stream, channel)` — deterministic lattice randomness.
    Hash,
    /// `sample_at(source, coordinates)` — reevaluate a field at another coordinate.
    SampleAt,
    /// `footprint(source)` — finite-difference scalar or vector sampling footprint.
    Footprint,
    /// `pixel_size()` — current sample spacing in document units.
    PixelSize,
    /// `over(foreground, background)` — linear premultiplied source-over compositing.
    Over,
    /// `mask(color, amount)` — clamp coverage and multiply premultiplied color.
    Mask,
    /// `opacity(color, amount)` — clamp opacity and multiply premultiplied color.
    Opacity,
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
/// List the complete catalog of operations implemented by both backends.
///
/// ```
/// let noise = synarith_core::operation("hash").expect("registered operation");
/// assert_eq!(noise.op, synarith_core::Op::Hash);
/// assert!(noise.inputs.iter().any(|input| input.name == "seed"));
/// ```
pub fn operations() -> Vec<Operation> {
    use Op::*;
    use ReturnType::{Exact as R, Input as I};
    let mut ops = vec![];
    macro_rules! add { ($op:ident,$name:literal,$cat:literal,$doc:literal,[$($arg:expr),*],$ret:expr) => { ops.push(Operation { op: $op, name:$name, category:$cat, description:$doc, inputs:vec![$($arg),*], result:$ret }); }; }
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
    add!(
        Atan2,
        "atan2",
        "Math",
        "Quadrant-aware arctangent in radians.",
        [f("y"), f("x")],
        R(Type::Float)
    );
    add!(
        Hash,
        "hash",
        "Randomness",
        "Deterministic seeded integer-lattice hash in [0,1).",
        [v("cell"), id("seed", 0), id("stream", 0), id("channel", 0)],
        R(Type::Float)
    );
    add!(
        SampleAt,
        "sample_at",
        "Sampling",
        "Reevaluate source at coordinates; propagate its sampling footprint.",
        [any("source"), v("coordinates")],
        I(0)
    );
    add!(
        Footprint,
        "footprint",
        "Sampling",
        "Scalar L1 central difference or vector maximum forward difference over one sample.",
        [n("source")],
        R(Type::Float)
    );
    add!(
        PixelSize,
        "pixel_size",
        "Sampling",
        "Current sample spacing in document units; zero for unfiltered point samples.",
        [],
        R(Type::Float)
    );
    add!(
        Over,
        "over",
        "Compositing",
        "Premultiplied linear-light source-over blend.",
        [c("foreground"), c("background")],
        R(Type::Color)
    );
    add!(
        Mask,
        "mask",
        "Compositing",
        "Multiply premultiplied color by clamped coverage.",
        [c("color"), f("amount")],
        R(Type::Color)
    );
    add!(
        Opacity,
        "opacity",
        "Compositing",
        "Multiply premultiplied color by clamped opacity.",
        [c("color"), f("amount")],
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
