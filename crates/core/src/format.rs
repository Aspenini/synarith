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
//! declaration = "seed" u32 ";"
//!             | "viewbox" "(" number "," number "," number "," number ")" ";"
//!             | "param" identifier ":" type "=" constant ";"
//!             | "let" identifier "=" expression ";"
//!             | "output" identifier "=" expression ";"
//!             | "group" identifier "(" typed-arguments? ")" "->" type
//!                   "{" let-declaration* "return" expression ";" "}"
//!             | "editor" json-string ";"
//! type = "float" | "int" | "bool" | "vec2" | "color"
//! ```
//!
//! There must be at least one named color output. Names are unique within their namespace; output
//! names are separate from expression names. Groups and global values share a declaration
//! namespace. Built-in function names may also be value names (`param scale: float = 6.0;` is
//! valid), but cannot be group names. `p`, `t`, `pi`, `tau`, `true`, and `false` are reserved.
//!
//! Global and group-local references may point forward, but must form acyclic graphs. Groups have
//! typed inputs and exactly one typed result. Direct and indirect recursion are errors. Groups can
//! capture global parameters/fields; explicit inputs are recommended when reusing a group with
//! different sources. Every group is validated, including unused definitions.
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
//! results are sanitized to zero. Operations with count/radius controls clamp dynamic values to
//! their documented safe bounds; invalid known constant arguments are diagnosed before rendering.
//! Seeded noise uses a fixed integer hash. Transcendentals and platform arithmetic can produce
//! small differences; bitwise identity across platforms is not guaranteed.
//!
//! ## Procedural operation families
//!
//! - **Fractals:** `mandelbrot(p, iterations: 128)` iterates `z = z² + p` from zero
//!   for 1–1024 steps. It returns a smooth normalized escape count in `[0, 1)` or
//!   one if no escape is observed. Its escape radius is 16. Finite iterations
//!   approximate set membership; f32 precision limits deep zooms. Use supersampling
//!   for boundary detail. Long boundary orbits can amplify tiny CPU/GPU rounding
//!   differences, producing different escape counts near the set edge.
//!   See `examples/mandelbrot.syar` for a colored, animated view.
//! - **Math and logic:** arithmetic nodes, trigonometry, power, exponential/logarithmic functions,
//!   clamps, steps, interpolation, conversions, comparisons, boolean operations, vector
//!   length/dot/normalization.
//! - **Noise:** quintic-interpolated value noise, gradient noise, cellular distance, fractal noise,
//!   turbulence, ridged noise, and integral-period lattice noise. Fractals use 1–12 octaves.
//!   Cellular uses a bounded 3×3 neighborhood. `periodic_noise` takes positive integral lattice
//!   periods.
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
//!   mirroring, polar mapping, arbitrary coordinate sampling with `transform(source,coordinates)`,
//!   and `warp(source,displacement)`.
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
//! network, process, include, plugin, or arbitrary code execution is part of the language.
//!
//!
//! ## Parse and inspect a document
//!
//! ```
//! use synarith_core::{parse, compile};
//! let document = parse(r#"syar 1;
//! param radius: float = 0.2;
//! let distance = circle(p - vec2(0.5, 0.5), radius);
//! output main = fill(distance, rgba(0.3, 0.7, 1.0));
//! "#)?;
//! let program = compile(&document)?;
//! assert_eq!(program.defaults()[0].data[0], 0.2);
//! assert_eq!(program.outputs.len(), 1);
//! # Ok::<(), synarith_core::Diagnostic>(())
//! ```
