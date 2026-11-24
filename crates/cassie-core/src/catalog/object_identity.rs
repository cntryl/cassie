//! Persistent object identifiers shared by the catalog and storage.

use crate::types::object_id::FIRST_NORMAL_OBJECT_ID;

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
    // The upper half of the OID space never meets an allocator identity
    // (`FIRST_NORMAL_OBJECT_ID` upward), and its width makes a name collision
    // between two indexes negligible. `CREATE INDEX` assigns an allocator
    // identity instead; this hash only names an index stored without one.
    let oid_range = u64::from(u32::MAX - 0x8000_0000);
    u32::try_from(hash % oid_range).unwrap_or(0) | 0x8000_0000
}
