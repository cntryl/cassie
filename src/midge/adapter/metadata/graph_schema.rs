//! Atomic publication of the reserved `cassie_graph` schema (#1268).
use super::composite_types::validate_composite_type;
use super::{key_encoding, CassieError, Midge};
use crate::app::DiagnosticError;
use crate::catalog::graph_schema::{build_graph_types, graph_schema_namespace};
use crate::catalog::{CompositeTypeMeta, NamespaceMeta};
use crate::types::graph::GRAPH_SCHEMA_OWNER;

impl Midge {
    /// Creates the system-owned `cassie_graph` namespace and its three
    /// composite types (with their array and relation OIDs) in one transaction:
    /// the namespace, its registry entry, every type and the identity counter
    /// become visible together or not at all.
    ///
    /// # Errors
    /// Returns `42710` when a namespace of that name already exists (it is
    /// never taken over) and any storage error; nothing is then published.
    pub fn publish_graph_schema(
        &self,
        database: &str,
    ) -> Result<(NamespaceMeta, Vec<CompositeTypeMeta>), CassieError> {
        let canonical = self.canonical_namespace_name(&graph_schema_namespace(database));
        let mut tx = self.begin_database_adapter_tx(database, cntryl_midge::TransactionMode::ReadWrite)?;
        tx.set_conflict_policy(cntryl_midge::ConflictPolicy::AbortOnWriteConflict);
        let namespace_key = Self::namespace_key(&canonical);
        if tx.get(&namespace_key).map_err(CassieError::from)?.is_some() {
            return Err(DiagnosticError::new(
                "42710",
                format!("schema \"{}\" already exists", crate::types::graph::GRAPH_SCHEMA),
            )
            .into());
        }
        let mut namespace = NamespaceMeta::new(&canonical, None);
        namespace.object_id = Self::allocate_object_id_to_tx(&mut tx)?;
        namespace.system_owner = Some(GRAPH_SCHEMA_OWNER.to_owned());
        let types = build_graph_types(database, || Self::allocate_object_id_to_tx(&mut tx))?;
        for metadata in &types {
            validate_composite_type(metadata)?;
            let value = serde_json::to_vec(metadata).map_err(|error| {
                CassieError::Parse(format!("serialize composite type metadata: {error}"))
            })?;
            tx.put(key_encoding::composite_type_key(&metadata.name), value, None)
                .map_err(CassieError::from)?;
        }
        let serialized = serde_json::to_vec(&namespace)
            .map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(namespace_key, serialized, None).map_err(CassieError::from)?;
        let mut namespaces = Self::load_namespaces(&tx)?;
        namespaces.push(canonical);
        namespaces.sort();
        namespaces.dedup();
        Self::save_namespaces(&mut tx, &namespaces)?;
        tx.commit(self.write_options_sync()).map_err(CassieError::from)?;
        Ok((namespace, types))
    }
}
