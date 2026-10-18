//! # Synarith language and compiler
//!
//! Read the [format guide](mod@format) for the `.syar` language. [`parse`] retains the
//! original source in a [`Document`], including comments and ignored legacy layout
//! metadata. [`compile`] resolves names, checks types and resource bounds, rejects
//! cycles, and expands reusable groups into a typed [`Program`]. Mathematical
//! functions compile to structured control flow with bounded iteration and
//! sample-local state; expression and call graphs remain acyclic.
//!
//! ```
//! use synarith_core::{compile, parse};
//! let document = parse("syar 1;use std;param radius:float=0.2;output main=fill(circle(p,radius));")?;
//! let program = compile(&document)?;
//! assert_eq!(program.parameters[0].name, "radius");
//! assert!(program.outputs.contains_key("main"));
//! # Ok::<(), synarith_core::Diagnostic>(())
//! ```
//!
//! # Editing and diagnostics
//!
//! [`Span`] indexes UTF-8 bytes; [`Diagnostic`] also reports one-based line and
//! character-column positions. [`Document::replace_expression`] makes a surgical
//! source edit and reparses it; [`format_source`] is a separate conservative action.
//! Call [`compile`] after edits to perform semantic validation.
//!
//! # Values and operation discovery
//!
//! [`Value`] uses four f32 components: scalars are splatted, vectors occupy X/Y,
//! and colors are linear premultiplied RGBA. [`operations`] is the authoritative
//! catalog of names, signatures, defaults, and descriptions for both renderers.
//! Parameters and outputs are discoverable on [`Program`]; parameter ordering is
//! declaration order and output ordering is alphabetical.
//!
//! Only compiler-produced graphs satisfy renderer invariants. Consumers should
//! inspect graph fields without introducing invalid node indices or cycles.
#![deny(missing_docs)]

mod compiler;
mod execution;
pub mod format;
mod language;
mod registry;
mod standard;

pub use compiler::*;
pub use execution::*;
pub use language::*;
pub use registry::*;
pub use standard::*;
