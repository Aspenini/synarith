//! # Synarith standard library
//!
//! The middle layer between user documents and the compiler/runtime. All image
//! algorithms in this crate are ordinary `.syar` source: noise, shapes, paths,
//! gradients, transforms, blend modes, and effects. This crate has no dependencies
//! and contains no renderer, parser, native algorithm implementation, or I/O.
//!
//! The compiler implements `use std;` by resolving only the required source
//! definitions. Documents can instead embed those definitions for portability.
//! Applications usually access this through `synarith::standard_library_source`.
#![deny(missing_docs)]

/// Portable standard-library source for the `syar 1;` language.
///
/// This is declarative source data, compiled through the same path as user code.
/// Changes to parameters, globals, time, or the render seed never modify it.
#[doc = concat!("\n```text\n", include_str!("standard.syar"), "\n```\n")]
pub fn standard_library_source() -> &'static str {
    include_str!("standard.syar")
}
