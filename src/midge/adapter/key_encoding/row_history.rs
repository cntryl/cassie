//! Point-mutation authority uses fixed binary relation boundaries.

use std::mem::size_of;

use super::{BASELINE, FAMILY_COLUMN_STORE, FAMILY_LEGACY_DOC, FAMILY_ROW, ROOT};

pub(in crate::midge::adapter) fn is_authoritative_row_key(key: &[u8]) -> bool {
    let Some(key) = key
        .strip_prefix(ROOT)
        .and_then(|key| key.strip_prefix(b"\0"))
        .and_then(|key| key.strip_prefix(BASELINE))
        .and_then(|key| key.strip_prefix(b"\0"))
    else {
        return false;
    };
    if [FAMILY_ROW, FAMILY_LEGACY_DOC].into_iter().any(|family| {
        key.strip_prefix(family)
            .is_some_and(|suffix| suffix.starts_with(b"\0"))
    }) {
        return true;
    }
    key.strip_prefix(FAMILY_COLUMN_STORE)
        .and_then(|suffix| suffix.strip_prefix(b"\0"))
        .and_then(|suffix| suffix.get(size_of::<u64>()..))
        .is_some_and(|suffix| suffix.starts_with(b"\0r\0"))
}
