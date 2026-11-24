//! Independent, deliberately naive oracle for graph path-pattern semantics
//! (#1257, `docs/graph-v060-contract.md`).
//!
//! The oracle depends only on `std` and shares no code with `src/`, so an
//! executor defect cannot also hide inside the oracle. It prefers exhaustive
//! depth-first enumeration to anything clever; performance is irrelevant.
//!
//! Include it from a test crate with
//! `#[path = "support/graph_oracle/mod.rs"] mod graph_oracle;`.
#![allow(dead_code)]

pub mod enumerate;
pub mod model;
pub mod pattern;
pub mod select;
pub mod weighted;

#[cfg(test)]
mod tests;
