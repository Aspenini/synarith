//! # Synarith text format, version 1
//!
//! ## Document structure
//!
//! Files are self-contained UTF-8 text with a required `syar 1;` header. Unsupported versions are
//! errors. Identifiers use ASCII letters, digits, and underscores, starting with a letter or
//! underscore. Whitespace, `//` line comments, and `/* … */` block comments are permitted between
//! tokens. Strings use JSON escaping. Semicolons terminate declarations.
//!
//! ```text
//! syar 1;
//! use std;
//! seed 4294967295;
//! viewbox (-1.0, -0.5, 2.0, 1.0);
//!
//! param tint: color = srgb("#75ecd9cc");
//! param radius: float = 0.2;
//! param enabled: bool = true;
//!
//! group badge(q: vec2, paint: color) -> color {
//!     let distance = circle(q, radius);
//!     return fill(distance, paint);
//! }
//!
//! let position = rotate(p, t * 0.2);
//! let foreground = badge(position, tint);
//! output main = select(enabled, foreground, rgba(0, 0, 0, 0));
//! ```
//!
//! Grammar outline:
//!
//! ```text
//! document  = "syar" "1" ";" declaration+
//! declaration = "use" "std" ";"
//!             | "seed" u32 ";"
//!             | "viewbox" "(" number "," number "," number "," number ")" ";"
//!             | "param" identifier ":" type "=" constant ";"
//!             | "let" identifier "=" expression ";"
//!             | "output" identifier "=" expression ";"
//!             | "group" identifier "(" typed-arguments? ")" "->" type
//!                   "{" let-declaration* "return" expression ";" "}"
//!             | "fn" identifier "(" typed-arguments? ")" "->" type block
//!             | "editor" json-string ";"
//! typed-argument = identifier ":" type ("=" constant)?
//! type = "float" | "int" | "bool" | "vec2" | "color"
//! ```
//!
//! There must be at least one named color output. Names are unique within their namespace; output
//! names are separate from expression names. Groups and global values share a declaration
//! namespace together with functions. Built-in function names may also be value names
//! (`param scale: float = 6.0;` is valid), but cannot name groups/functions. Built-in variables,
//! boolean literals, declaration keywords, and control-flow keywords are reserved.
//!
//! Global and group-local references may point forward, but must form acyclic graphs. Groups have
//! typed inputs and exactly one typed result. Direct and indirect recursion are errors. Groups can
//! capture global parameters/fields; explicit inputs are recommended when reusing a group with
//! different sources. Every group is validated, including unused definitions.
//!
//! ## Small core and source-defined library
//!
//! Only scalar/vector math, logic, color construction/channels/interpolation,
//! premultiplied alpha compositing, deterministic hashing, and field sampling are native
//! operations. [`crate::operations`] lists the complete core. Noise algorithms, shapes,
//! gradients, paths, transforms, blend modes, and effects are ordinary typed Synarith
//! functions/groups in [`crate::standard_library_source`]. `use std;` explicitly opts into
//! that bundled source library. Only reachable definitions are added; unused helpers do not
//! consume graph limits. Local definitions can replace library helpers. No file lookup,
//! network access, external imports, or hidden native algorithm substitution occurs.
//!
//! [`crate::expand_standard_library`] and `syar fmt FILE --expand-std` embed the required
//! definitions and remove the directive, producing a standalone document. Original comments
//! and layout remain intact. The expanded source follows the same type, recursion, loop,
//! node, and evaluation limits as handwritten definitions. Errors in imported definitions
//! are reported at the `use std;` directive; errors in document expressions retain their
//! original positions. Mandelbrot and other named fractals are never implicitly provided.
//!
//! Both functions and groups accept optional constant argument defaults, e.g.
//! `fn circle(q: vec2, radius: float = 0.3) -> float { return length(q)-abs(radius); }`.
//! Defaults use the parameter-constant grammar, are checked even when unused, and may be
//! omitted with positional or named calls. Each argument is supplied at most once.
//!
//! ### Compilation and runtime layers
//!
//! `synarith-stdlib` owns portable algorithm source and has no dependencies. The
//! compiler owns parsing, source-preserving expansion, typing, bounds, and lowering.
//! [`crate::ExecutionPlan`] schedules reachable expression blocks in dependency order,
//! reusing a shared node within one expression. Source samplers and their coordinate
//! expressions have independent blocks. Each statement reevaluates its values after local
//! mutation; function calls and resampling create separate register contexts.
//! All source definitions are still semantically validated, including unused ones.
//!
//! Both backends consume this schedule and the same structured function instructions.
//! CPU execution uses registers where sharing justifies them and direct evaluation
//! for small trees. GPU emission uses explicit temporaries and native shader loops.
//! Loops are never expanded into thousands of copied expression nodes. Prepared
//! CPU schedules and GPU pipelines are reusable across time, parameters, and tiles.
//! CPU code remains interpreted; no machine-code JIT is included. A plan is limited
//! to 262144 scheduled operations, independently of source and per-sample work limits.
//!
//! ### Sampling primitives
//!
//! `sample_at(source, coordinates)` reevaluates a field at arbitrary document coordinates.
//! Its footprint is propagated using forward differences of the coordinate expression.
//! `pixel_size()` is the current scalar sample spacing in document units (zero for an
//! unfiltered point sample). `footprint(source)` returns zero at zero spacing; otherwise
//! it measures the absolute X plus absolute Y central differences for a scalar, or the
//! maximum X/Y forward-difference vector length for `vec2`. Differencing evaluates the
//! source with zero footprint, preventing recursive derivative estimation.
//!
//! Groups preserve field expressions, so a blur group can call `sample_at` repeatedly.
//! Functions evaluate arguments once, so expensive sampled values should be passed to a
//! function for reuse. Sampling cannot recover spatial variation from an already evaluated
//! function-local value; wrap the complete function call in `sample_at` or a filter group.
//! `footprint` and `pixel_size` describe sampling, not an image's composition or view box.
//!
//! ### Deterministic randomness
//!
//! `hash(cell, seed: 0, stream: 0, channel: 0)` floors XY and clamps them to the f32-representable
//! i32 range [-2147483648, 2147483520]. All hash arithmetic wraps in u32. The effective seed
//! is `((document_seed XOR u32(seed)) + u32(stream)) XOR (u32(channel) * 0x68bc21eb)`.
//! The lattice word is `u32(x)*0x9e3779b9 XOR u32(y)*0x85ebca6b XOR effective_seed`.
//! Avalanche steps are `v ^= v >> 16; v *= 0x7feb352d; v ^= v >> 15; v *= 0x846ca68b;
//! v ^= v >> 16`. The top 24 bits divided by 16777216 produce a value in [0,1).
//! Seed overrides replace the full document u32 seed; integer arguments use the checked
//! language integer range. CPU/GPU share the exact algorithm. Continuous noise,
//! interpolation, octave accumulation, and cellular searches are implemented in `.syar`.
//!
//! ## Mathematical functions and iteration
//!
//! Pure `fn` definitions support typed arguments/results, immutable `let`, mutable `var`,
//! assignment (`=`, `+=`, `-=`, `*=`, `/=`), `if`/`else if`/`else`, bounded loops, `break`,
//! `continue`, and early `return`. They may call other functions, groups, and built-ins.
//! Every definition is validated, including unused ones. Direct/indirect recursion is rejected.
//!
//! ```
//! use synarith_core::{parse, compile};
//! let program = compile(&parse(r##"syar 1;use std;
//! fn julia(q: vec2, c: vec2, steps: int) -> float {
//!     var z = q;
//!     for i in 0..256 {
//!         if i >= steps { break; }
//!         z = vec2(x(z)*x(z)-y(z)*y(z), 2.0*x(z)*y(z)) + c;
//!         if dot(z,z) > 256.0 { return float(i) / float(steps); }
//!     }
//!     return 1.0;
//! }
//! output main = mix(srgb("#071326"), srgb("#efc68b"), julia(p, vec2(-0.72,0.19), 80));
//! "##)?)?;
//! assert_eq!(program.functions.len(), 1);
//! assert!(program.nodes.len() < 80); // The loop body is compiled only once.
//! # Ok::<(), synarith_core::Diagnostic>(())
//! ```
//!
//! ```text
//! block     = "{" statement* "}"
//! statement = ("let" | "var") identifier (":" type)? "=" expression ";"
//!           | identifier ("=" | "+=" | "-=" | "*=" | "/=") expression ";"
//!           | "if" expression block ("else" (block | if-statement))?
//!           | "for" identifier "in" integer-expression ".." integer-expression block
//!           | "while" expression "limit" integer-expression block
//!           | ("break" | "continue") ";"
//!           | "return" expression ";"
//! ```
//!
//! Local types are inferred unless annotated. Assignment preserves a variable's type, permitting
//! integer-to-float promotion. Arguments and loop indices are immutable. Bindings are visible
//! after declaration and inside nested blocks; block-local names do not leak, and locals cannot
//! shadow another visible local. Every path must return the declared type. Unreachable statements
//! following a guaranteed exit are errors.
//!
//! `for` ranges are half open, with `0 <= start <= end <= 4096`. `while condition limit N` tests
//! the boolean condition before each step and executes at most `N` times; exhausting the limit
//! exits normally. Bounds must be compile-time integer arithmetic, including immutable integer
//! constants. For a dynamic iteration count, use a constant bound and `if i >= steps { break; }`.
//!
//! ```text
//! var estimate = 1.0;
//! var error = 1.0;
//! while error > 0.00001 limit 64 {
//!     let next = 0.5 * (estimate + 2.0 / estimate);
//!     error = abs(next - estimate);
//!     estimate = next;
//! }
//! ```
//!
//! Documents accept at most 128 functions and 4096 total function instructions. Each function
//! accepts at most 16 arguments and 128 local slots, including arguments and nested-block
//! bindings. CPU and WGSL execute structured loop bodies directly, without graph unrolling.
//! Worst-case work, including nested loops, calls, conditions, and source sampling, counts toward
//! the two-million-step sample budget. All state belongs to one call at one sample; there is no
//! shared mutation, frame history, I/O, or imported executable shader code.
//!
//! Function locals hold evaluated values; sampling a local again within its call reads that
//! stored value. To resample an algorithm spatially, wrap its complete call, for example
//! `blur(field(p), 0.01)` or `sample_at(field(p), p * 2.0)`. The whole function is then reevaluated
//! at each requested coordinate. Footprint inference cannot recover derivatives through stored
//! iterative values; use supersampling for high-frequency custom algorithms.
//! See `examples/algorithms.syar` and the volumetric `examples/deep-space.syar` wallpaper.
//!
//! ## Expressions and types
//!
//! Expressions include numeric/boolean literals, named fields, parenthesized expressions, calls,
//! unary `+ - !`, arithmetic `+ - * / %`, comparisons `< <= > >= == !=`, and logical `&& ||`.
//! Precedence follows conventional arithmetic, comparison, and logical ordering. Calls accept
//! positional arguments followed by named arguments; duplicate, unknown, or missing required
//! arguments are errors. Calls permit a trailing comma.
//!
//! Numeric literals without a decimal point or exponent are integers. Integers are represented
//! exactly within `[-16777216,16777216]` in the evaluator's four-component f32 representation. The
//! document seed is a separate exact unsigned 32-bit value. Integers promote to floats; floats
//! require an explicit `int()` conversion when an integer input is expected. Integer division
//! produces a float. Scalars broadcast in arithmetic with vectors or colors; vectors and colors
//! cannot be mixed arithmetically.
//!
//! Parameters have constant defaults: literals, unary signed literals, `vec2()`, `rgba()`, or
//! `srgb()`. Parameters cannot depend on time, position, or other fields. Overrides retain the
//! declared type. Colors are premultiplied linear RGBA internally; public color parameter values
//! use the same representation.
//!
//! `vec2(x,y)` constructs a coordinate/vector. `rgba(r,g,b,a)` takes **straight linear** RGB,
//! clamps alpha to `[0,1]`, and premultiplies it; alpha defaults to one. `srgb("#RRGGBB")` and
//! `srgb("#RRGGBBAA")` decode sRGB channels to linear light and premultiply by alpha. sRGB hex
//! literals must use exactly six or eight hexadecimal digits.
//!
//! `mix(a,b,amount)` interpolates compatible fields. `select(condition,a,b)` selects the true or
//! false field. Component and channel extractors are `x`, `y`, `red`, `green`, `blue`, and `alpha`;
//! RGB channel extraction unpremultiplies the color.
//!
//! Obtain the authoritative operation signatures, defaults, categories, and descriptions with:
//!
//! ```sh
//! syar inspect --operations
//! ```
//!
//! Rust callers can inspect [`crate::operations`], [`crate::Operation`], and [`crate::Op`].
//!
//! ## Coordinates, sampling, color, and animation
//!
//! `p` is a continuous document-space coordinate, independent of export dimensions. The default
//! view box is `(0,0,1,1)`, with X rightward and Y downward. Shapes centered at the origin can be
//! positioned with `p - vec2(0.5,0.5)` or a translate node. A view box is `(x,y,width,height)` with
//! strictly positive dimensions. Renderers preserve its aspect ratio and use transparent padding.
//!
//! `t` is time in seconds supplied by the host. There is no mutable frame state. `loop(t,duration)`
//! returns a phase in `[0,1)`; `pulse(phase)` returns a smooth periodic pulse. A looped document
//! must use that phase consistently; wrapping a linear translation does not automatically make its
//! content seamless. Seeded noise is stable across sampling order, tile boundaries, and frame
//! seeking.
//!
//! Supersampling uses a deterministic square sample grid, with 1–8 samples **per axis**. Shape
//! coverage uses the local distance footprint; value/gradient and fractal noise filter unresolved
//! frequencies. Sampling transforms propagate a scalar footprint estimate. These filters improve
//! sampling but do not promise perfect analytic antialiasing of every arbitrary nonlinear field.
//!
//! All compositing and effects operate in linear light with premultiplied alpha.
//! `over(foreground,background)` is source-over. `multiply`, `screen`, and `overlay` use the same
//! source-over alpha treatment. PNG pixels are converted to straight sRGB RGBA8; RGBA32F pixels
//! remain linear and premultiplied.
//!
//! Runtime division by an effectively zero denominator yields zero. Invalid/nonfinite numeric
//! results are sanitized to zero. Invalid known constant core mathematical domains are diagnosed
//! before rendering. Source-library algorithms clamp count/radius controls as specified in their
//! code, equally for constant and dynamic inputs.
//! Seeded noise uses a fixed integer hash. Transcendentals and platform arithmetic can produce
//! small differences; bitwise identity across platforms is not guaranteed.
//!
//! ## Source-library algorithm families
//!
//! Image algorithms are written in `.syar`. The families below require `use std;` or their
//! explicit source definitions, rather than native opcodes. For example,
//! `examples/mandelbrot.syar` defines `mandelbrot(c, iterations)` as an ordinary typed function:
//! a radius-16 escape test, bounded iteration of `z = z² + c`, and logarithmic smoothing.
//! The example preserves its animated palette and clamps its own count to 1–1024.
//! Copy its definition into documents that need it; calling an undefined `mandelbrot()` is an
//! unknown-operation error. Julia, Barnsley fern, and nebula integration likewise live in their
//! example documents. Finite iteration approximates membership, and f32 precision limits zooms;
//! long boundary orbits can amplify small CPU/GPU arithmetic differences.
//!
//! - **Math and logic:** arithmetic nodes, trigonometry, power, exponential/logarithmic functions,
//!   clamps, steps, interpolation, conversions, comparisons, boolean operations, vector
//!   length/dot/normalization.
//! - **Noise:** quintic-interpolated value noise, gradient noise, cellular distance, fractal noise,
//!   turbulence, ridged noise, and integral-period lattice noise. Fractals use 1–12 octaves.
//!   Cellular uses a bounded 3×3 neighborhood. `periodic_noise` takes positive integral lattice
//!   periods, rounded and clamped to at least one by the library.
//! - **Gradients:** linear projection, radial distance, angular position, and a four-stop color
//!   ramp. Ramp endpoints are 0 and 1, with two movable internal stops `p1` and `p2`.
//! - **Shapes:** circle, half-extent rectangle, rounded rectangle, approximate ellipse distance,
//!   regular polygon (3–64 sides), and star (2–32 points). Combine distances with
//!   union/intersection/subtraction/smooth union.
//! - **Paths:** `bezier` is cubic stroke distance using 64 segments. `path` is an open or closed
//!   cubic path with a document-unit tolerance and 8–256 segments selected from curvature. Closing
//!   adds a straight segment to the start and uses even/odd interior classification.
//!   `fill(path(...))` fills it; `stroke(path(...),width)` outlines it. Multiple fields can be
//!   combined through distance operations. Tolerance has a practical bound at the segment cap.
//! - **Coordinates and resampling:** translation, rotation, per-axis scaling, centered repetition,
//!   mirroring, polar mapping, arbitrary coordinate sampling with `sample_at(source,coordinates)`,
//!   and a scalar `warp(source,displacement)` helper. Use `sample_at` directly for other types.
//! - **Effects:** signed-distance fill/stroke/exterior outline/glow, opacity and masking,
//!   straight-RGB inversion, linear grayscale/contrast/saturation, bounded 3×3 Gaussian blur, and
//!   height-derived normal color. Blur radius and derivative epsilon are in document units. No
//!   operation imports or caches a raster asset as document content.
//!
//! ## Metadata, preservation, and limits
//!
//! For compatibility, version 1 accepts legacy `editor` layout metadata as a JSON object encoded in
//! a JSON string. It has no effect on compilation or rendering. Text-preserving library operations
//! retain its contents.
//!
//! ```text
//! editor "{\"positions\":{\"::clouds\":[210,0]},\"zoom\":0.8}";
//! ```
//!
//! Use [`crate::Document::replace_expression`] to replace an expression while retaining surrounding
//! comments and formatting. Explicit `fmt` conservatively normalizes line endings outside strings
//! and ensures a final newline; it does not regenerate the document from the typed graph.
//!
//! Limits: 4 MiB source, 128 parser nesting levels, 4096 compiled nodes, 96 graph levels, and an
//! estimated expanded evaluation budget of two million per sample. Compilation includes reusable
//! group validation. Individual float render regions are capped at 1 GiB; streamed PNG export uses
//! smaller strips. Dimensions are 1–1048576 per axis, subject to resource and PNG limits. No file,
//! network, process, include, plugin, or external code execution is part of the language.
//!
//!
//! ## Parse and inspect a document
//!
//! ```
//! use synarith_core::{parse, compile};
//! let document = parse(r#"syar 1;use std;
//! param radius: float = 0.2;
//! let distance = circle(p - vec2(0.5, 0.5), radius);
//! output main = fill(distance, rgba(0.3, 0.7, 1.0));
//! "#)?;
//! let program = compile(&document)?;
//! assert_eq!(program.defaults()[0].data[0], 0.2);
//! assert_eq!(program.outputs.len(), 1);
//! # Ok::<(), synarith_core::Diagnostic>(())
//! ```
