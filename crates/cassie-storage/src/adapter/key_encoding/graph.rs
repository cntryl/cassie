use super::{key, prefix, scoped_key, FAMILY_DATABASE_IDENTITY, FAMILY_PROPERTY_GRAPH};

/// Every mapped property graph definition in one database family.
pub(crate) fn property_graph_prefix() -> Vec<u8> {
    prefix(FAMILY_PROPERTY_GRAPH, &[])
}

pub(crate) fn property_graph_key(canonical_name: &str) -> Vec<u8> {
    let normalized = canonical_name.trim().to_ascii_lowercase();
    scoped_key(FAMILY_PROPERTY_GRAPH, &normalized, &[])
}

/// The durable identity of the database that owns a Data family.
pub(crate) fn database_identity_key() -> Vec<u8> {
    key(FAMILY_DATABASE_IDENTITY, &[])
}

/// The family of the removed `CREATE GRAPH` definitions.
const LEGACY_FAMILY_GRAPH: &[u8] = b"graph";
/// The family of the removed `CREATE GRAPH` adjacency entries.
const LEGACY_FAMILY_GRAPH_ADJACENCY: &[u8] = b"\x16";

/// Prefixes of the state the removed `CREATE GRAPH` feature persisted. Nothing
/// reads or writes them any more; startup deletes whatever a store still holds.
pub(crate) fn legacy_graph_prefixes() -> [Vec<u8>; 2] {
    [
        prefix(LEGACY_FAMILY_GRAPH, &[]),
        prefix(LEGACY_FAMILY_GRAPH_ADJACENCY, &[]),
    ]
}

/// Returns the persisted prefixes used by the removed graph feature.
///
/// This is exposed for compatibility diagnostics that must inspect legacy
/// state without maintaining a second copy of the storage key encoding.
#[doc(hidden)]
#[must_use]
pub fn legacy_graph_prefixes_for_diagnostics() -> [Vec<u8>; 2] {
    legacy_graph_prefixes()
}

/// One definition key and one adjacency key shaped like the removed feature wrote them.
/// Returns representative persisted keys used by the removed graph feature.
#[doc(hidden)]
#[must_use]
pub fn legacy_graph_sample_keys_for_diagnostics() -> [Vec<u8>; 2] {
    [
        scoped_key(LEGACY_FAMILY_GRAPH, "postgres.public.social", &[]),
        key(LEGACY_FAMILY_GRAPH_ADJACENCY, &[b"7", b"M"]),
    ]
}
