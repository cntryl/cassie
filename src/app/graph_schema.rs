//! Bootstrap of the reserved `cassie_graph` schema (#1268).
//!
//! The bootstrap inspects the catalog first. A schema of that name without the
//! system ownership marker is a user object and fails with `42710`, leaving
//! every existing object, owner and permission untouched. Otherwise the schema,
//! its composite descriptors, type and array OIDs and relation OIDs are
//! published in one storage transaction, so a failed bootstrap leaves none of
//! them visible. The call is idempotent: a system-owned schema is a no-op.
use super::{Cassie, CassieError, DiagnosticError};
use crate::catalog::graph_schema::GraphSchemaState;
use crate::types::graph::GRAPH_SCHEMA;

impl Cassie {
    /// Ensures the reserved `cassie_graph` schema and types exist in `database`.
    ///
    /// # Errors
    /// Returns `42710` when a user-owned schema holds the name (resolve it
    /// through the supported schema migration workflow, then retry), `XX000`
    /// when the system-owned schema is incomplete, and storage errors.
    pub fn ensure_graph_schema(&self, database: &str) -> Result<(), CassieError> {
        match self.catalog.graph_schema_state(database) {
            GraphSchemaState::SystemOwned => {
                if self.catalog.graph_descriptors(database).is_some() {
                    Ok(())
                } else {
                    Err(DiagnosticError::new(
                        "XX000",
                        format!("reserved schema {GRAPH_SCHEMA} is missing its graph types"),
                    )
                    .into())
                }
            }
            GraphSchemaState::UserOwned => Err(DiagnosticError::new(
                "42710",
                format!("schema \"{GRAPH_SCHEMA}\" already exists and is not owned by Cassie"),
            )
            .with_hint(format!(
                "{GRAPH_SCHEMA} is reserved for graph types. Rename or drop the existing schema through your schema migration workflow, then retry."
            ))
            .into()),
            GraphSchemaState::Absent => {
                let (namespace, types) = self.midge.publish_graph_schema(database)?;
                self.catalog.register_namespace_meta(namespace);
                for metadata in types {
                    self.catalog.register_composite_type(metadata);
                }
                self.bump_schema_epoch_and_invalidate_query_cache()?;
                Ok(())
            }
        }
    }

    /// Bootstraps every known database at startup. A user-owned schema of the
    /// reserved name does not stop the server (the operator must be able to
    /// resolve it with SQL); the graph feature stays unavailable in that
    /// database and a retry reports the conflict again.
    pub(super) fn bootstrap_graph_schemas(&self) -> Result<(), CassieError> {
        for database in self.catalog.list_databases() {
            match self.ensure_graph_schema(&database.name) {
                Ok(()) => {}
                Err(error) if is_name_conflict(&error) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }
}

fn is_name_conflict(error: &CassieError) -> bool {
    matches!(error, CassieError::Diagnostic(diagnostic) if diagnostic.sql_state == "42710")
}
