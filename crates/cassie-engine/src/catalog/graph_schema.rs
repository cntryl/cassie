//! The reserved `cassie_graph` schema and its composite types (#1268).
//!
//! The schema is system owned: it carries [`GRAPH_SCHEMA_OWNER`] in its
//! namespace metadata. A same-named schema without the marker is a user object
//! and is never taken over. The three types are ordinary named composite types
//! whose descriptors, array types and backing `relkind = 'c'` relations are
//! allocated and persisted like any other composite.

use super::{canonical_relation_name, Catalog};
use crate::types::graph::{GraphDescriptors, GraphTypeKind, GRAPH_SCHEMA, GRAPH_SCHEMA_OWNER};

pub use cassie_core::catalog::graph_schema::{graph_schema_namespace, is_graph_schema};

/// What the catalog holds under the reserved schema name of one database.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphSchemaState {
    /// No schema of that name exists: bootstrap may create it.
    Absent,
    /// The system-owned schema exists.
    SystemOwned,
    /// A user-created schema holds the name: bootstrap must fail (`42710`).
    UserOwned,
}

/// The error of user DDL that would create, alter, rename or drop an object of
/// the reserved schema. The system owns the schema: PostgreSQL reports a
/// system-owned schema as `42501`, and so does Cassie.
#[must_use]
pub fn reserved_schema_error(action: &str) -> crate::app::CassieError {
    crate::app::DiagnosticError::new(
        "42501",
        format!("permission denied to {action}: schema \"{GRAPH_SCHEMA}\" is reserved for Cassie graph types"),
    )
    .into()
}

impl Catalog {
    /// Whether `schema` (local or qualified) of `database` is the system-owned
    /// reserved schema. A user-owned schema of the same name is not reserved:
    /// it is the operator's to rename or drop.
    #[must_use]
    pub fn is_reserved_graph_schema(&self, database: &str, schema: &str) -> bool {
        let (database, name) = match super::parse_name(schema) {
            Ok(super::ParsedName::Unqualified(name)) => (database.to_owned(), name),
            Ok(super::ParsedName::SchemaQualified { schema, name }) => (schema, name),
            Ok(super::ParsedName::DatabaseQualified {
                database, schema, ..
            }) => (database, schema),
            Err(_) => return false,
        };
        is_graph_schema(&name)
            && self.graph_schema_state(&database) == GraphSchemaState::SystemOwned
    }

    /// Who owns the reserved schema name in `database`.
    #[must_use]
    pub fn graph_schema_state(&self, database: &str) -> GraphSchemaState {
        match self.get_namespace(&graph_schema_namespace(database)) {
            None => GraphSchemaState::Absent,
            Some(namespace) if namespace.system_owner.as_deref() == Some(GRAPH_SCHEMA_OWNER) => {
                GraphSchemaState::SystemOwned
            }
            Some(_) => GraphSchemaState::UserOwned,
        }
    }

    /// The descriptors of the three reserved types of `database`, or `None`
    /// until the schema has been bootstrapped there.
    #[must_use]
    pub fn graph_descriptors(&self, database: &str) -> Option<GraphDescriptors> {
        if self.graph_schema_state(database) != GraphSchemaState::SystemOwned {
            return None;
        }
        let descriptor = |kind: GraphTypeKind| {
            self.get_composite_type(&canonical_relation_name(
                database,
                GRAPH_SCHEMA,
                kind.type_name(),
            ))
            .map(|metadata| metadata.descriptor())
        };
        Some(GraphDescriptors {
            key_part: descriptor(GraphTypeKind::KeyPart)?,
            element_ref: descriptor(GraphTypeKind::ElementRef)?,
            path: descriptor(GraphTypeKind::Path)?,
        })
    }
}
