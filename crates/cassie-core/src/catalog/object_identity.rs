//! Persistent object identifiers shared by the catalog and storage.

use crate::types::object_id::FIRST_NORMAL_OBJECT_ID;

const LEGACY_INDEX_OID_START: u32 = 0x8000_0000;

/// Object identities are allocated from one counter, offset into
/// PostgreSQL's user OID range.
#[must_use]
pub fn normal_oid(object_id: u64) -> Option<u32> {
    if object_id == 0 {
        return None;
    }
    u32::try_from(object_id)
        .ok()
        .and_then(|id| id.checked_add(FIRST_NORMAL_OBJECT_ID))
}

/// The name-derived catalog OID of an index relation stored without an
/// assigned identity.
#[must_use]
pub fn stable_index_oid(name: &str) -> u32 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in "index"
        .bytes()
        .chain(*b":")
        .chain(name.to_ascii_lowercase().bytes())
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    let oid_range = u64::from(u32::MAX - LEGACY_INDEX_OID_START);
    u32::try_from(hash % oid_range).unwrap_or(0) | LEGACY_INDEX_OID_START
}

/// Finds the first unused legacy index OID, starting at its name-derived candidate.
///
/// # Panics
///
/// Panics if the upper half of the OID space is exhausted.
pub fn available_legacy_index_oid(candidate: u32, mut occupied: impl FnMut(u32) -> bool) -> u32 {
    let first = candidate | LEGACY_INDEX_OID_START;
    let mut oid = first;
    while occupied(oid) {
        oid = oid.wrapping_add(1) | LEGACY_INDEX_OID_START;
        assert_ne!(oid, first, "legacy index OID space exhausted");
    }
    oid
}
