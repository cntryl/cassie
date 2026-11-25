//! The `cassie_graph` path carrier types (#1268).
//!
//! The reserved schema owns three named composite types: `graph_key_part`,
//! `graph_element_ref` and `graph_path`. PostgreSQL composite types cannot
//! enforce the tagged-part invariant, so every construction path funnels
//! through [`validate_record`]: SQL constructors, `ROW` and text casts, stored
//! column writes and bind parameters. The pure model, total order and JSON
//! export live in the submodules.

mod json;
mod model;
mod order;
mod validate;

pub use json::path_to_json;
pub use model::{Direction, ElementKind, ElementRef, GraphDescriptors, KeyPart, Path};
pub use order::{compare_element_refs, compare_paths};
pub use validate::{validate_record, GraphTypeError};

pub use cassie_core::types::graph_identity::{
    GraphTypeKind, ELEMENT_REF_TYPE, GRAPH_SCHEMA, GRAPH_SCHEMA_OWNER, KEY_PART_TYPE, PATH_TYPE,
};

#[cfg(test)]
mod tests;
