//! Key family of persisted enum type descriptors (#827).
use super::{key, prefix};

const FAMILY_ENUM_TYPE: &[u8] = b"enum-type";

pub(in crate::midge::adapter) fn enum_type_key(canonical_name: &str) -> Vec<u8> {
    key(
        FAMILY_ENUM_TYPE,
        &[canonical_name.to_ascii_lowercase().as_bytes()],
    )
}

pub(in crate::midge::adapter) fn enum_type_prefix() -> Vec<u8> {
    prefix(FAMILY_ENUM_TYPE, &[])
}
