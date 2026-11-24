//! The system-owned `cassie_graph` objects of a database are catalog state,
//! not user data: they do not make the database non-empty and they leave with
//! its family (#1268).
use super::database_catalog_rewrite::key_family;
use super::RawStorageEntry;
use crate::catalog::{CompositeTypeMeta, NamespaceMeta};
use cassie_core::types::graph_identity::GRAPH_SCHEMA_OWNER;

pub(crate) fn owns_reserved_schema(entries: &[RawStorageEntry]) -> bool {
    entries
        .iter()
        .filter(|(key, _)| key_family(key) == Some("namespace"))
        .filter_map(|(_, value)| serde_json::from_slice::<NamespaceMeta>(value).ok())
        .any(|namespace| namespace.system_owner.as_deref() == Some(GRAPH_SCHEMA_OWNER))
}

pub(crate) fn is_reserved_entry(key: &[u8], value: &[u8]) -> bool {
    match key_family(key) {
        Some("namespace") => serde_json::from_slice::<NamespaceMeta>(value)
            .is_ok_and(|namespace| namespace.system_owner.as_deref() == Some(GRAPH_SCHEMA_OWNER)),
        Some("composite-type") => {
            serde_json::from_slice::<CompositeTypeMeta>(value).is_ok_and(|metadata| {
                crate::catalog::graph_schema::is_graph_schema(&metadata.schema_name())
            })
        }
        _ => false,
    }
}

/// The durable database identity and the stored property graph definitions.
/// A distinct clone must not inherit them: it allocates its own database
/// identity, and graphs are recreated there with fresh graph and mapping
/// identities, so a copied reference never resolves in the clone (#1268).
pub(crate) fn is_clone_excluded_identity_entry(key: &[u8]) -> bool {
    matches!(
        key_family(key),
        Some("database-identity" | "property-graph")
    )
}

/// Removes the reserved schema from a `namespaces` registry value.
fn without_reserved_namespace(value: &[u8]) -> Vec<u8> {
    let Ok(names) = serde_json::from_slice::<Vec<String>>(value) else {
        return value.to_vec();
    };
    let kept = names
        .into_iter()
        .filter(|name| !crate::catalog::graph_schema::is_graph_schema(name))
        .collect::<Vec<_>>();
    serde_json::to_vec(&kept).unwrap_or_else(|_| value.to_vec())
}

/// `entries` without the reserved schema's namespace and types, when the
/// system owns that schema. A user-owned schema of the same name is user data.
pub(crate) fn without_reserved_objects(entries: Vec<RawStorageEntry>) -> Vec<RawStorageEntry> {
    if !owns_reserved_schema(&entries) {
        return entries;
    }
    entries
        .into_iter()
        .filter(|(key, value)| !is_reserved_entry(key, value))
        .map(|(key, value)| {
            if key_family(&key) == Some("namespaces") {
                let value = without_reserved_namespace(&value);
                (key, value)
            } else {
                (key, value)
            }
        })
        .collect()
}
