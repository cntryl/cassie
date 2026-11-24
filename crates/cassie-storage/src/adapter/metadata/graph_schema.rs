//! Atomic publication of the reserved `cassie_graph` schema (#1268).
use super::composite_types::validate_composite_type;
use super::{key_encoding, CassieError, Midge};
use crate::catalog::graph_schema::{build_graph_types, graph_schema_namespace};
use crate::catalog::{CompositeTypeMeta, NamespaceMeta};
use cassie_core::types::graph_identity::GRAPH_SCHEMA_OWNER;

impl Midge {
    /// Creates the system-owned `cassie_graph` namespace and its three
    /// composite types (with their array and relation OIDs) in one transaction
    /// of the database family: the namespace, its registry entry and every type
    /// become visible together or not at all. The identity counter lives in the
    /// schema family, so identities are drawn from it first; a failed publish
    /// leaves only unused gaps in the counter.
    ///
    /// # Errors
    /// Returns `42710` when a namespace of that name already exists (it is
    /// never taken over) and any storage error; nothing is then published.
    pub fn publish_graph_schema(
        &self,
        database: &str,
    ) -> Result<(NamespaceMeta, Vec<CompositeTypeMeta>), CassieError> {
        let canonical = self.canonical_namespace_name(&graph_schema_namespace(database));
        let mut tx =
            self.begin_database_adapter_tx(database, cntryl_midge::TransactionMode::ReadWrite)?;
        tx.set_conflict_policy(cntryl_midge::ConflictPolicy::AbortOnWriteConflict);
        let namespace_key = Self::namespace_key(&canonical);
        if tx.get(&namespace_key).map_err(CassieError::from)?.is_some() {
            return Err(CassieError::diagnostic(
                "42710",
                format!(
                    "schema \"{}\" already exists",
                    cassie_core::types::graph_identity::GRAPH_SCHEMA
                ),
            ));
        }
        let mut namespace = NamespaceMeta::new(&canonical, None);
        namespace.object_id = self.allocate_object_id()?;
        namespace.system_owner = Some(GRAPH_SCHEMA_OWNER.to_owned());
        let types = build_graph_types(database, || self.allocate_object_id())?;
        for metadata in &types {
            validate_composite_type(metadata)?;
            let value = serde_json::to_vec(metadata).map_err(|error| {
                CassieError::Parse(format!("serialize composite type metadata: {error}"))
            })?;
            tx.put(
                key_encoding::composite_type_key(&metadata.name),
                value,
                None,
            )
            .map_err(CassieError::from)?;
        }
        let serialized = serde_json::to_vec(&namespace)
            .map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(namespace_key, serialized, None)
            .map_err(CassieError::from)?;
        let mut namespaces = Self::load_namespaces(&tx)?;
        namespaces.push(canonical);
        namespaces.sort();
        namespaces.dedup();
        Self::save_namespaces(&mut tx, &namespaces)?;
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)?;
        Ok((namespace, types))
    }
}
