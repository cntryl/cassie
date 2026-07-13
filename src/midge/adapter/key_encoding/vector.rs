use cntryl_lexkey::LexKey;

use super::{
    data_scoped_key, data_scoped_prefix, hnsw_source_summary_key, FAMILY_VECTOR_INDEX_STATE,
};

pub(crate) fn ivfflat_source_summary_key(collection: &str, field: &str) -> Vec<u8> {
    hnsw_source_summary_key(collection, field)
}

pub(crate) fn ivfflat_membership_key(
    collection: &str,
    field: &str,
    list: usize,
    id: &str,
) -> Vec<u8> {
    let list = u64::try_from(list).unwrap_or(u64::MAX).to_be_bytes();
    data_scoped_key(
        FAMILY_VECTOR_INDEX_STATE,
        collection,
        &[field.as_bytes(), b"l", &list, id.as_bytes()],
    )
}

pub(crate) fn ivfflat_membership_prefix(collection: &str, field: &str) -> Vec<u8> {
    data_scoped_prefix(
        FAMILY_VECTOR_INDEX_STATE,
        collection,
        &[field.as_bytes(), b"l"],
    )
}

pub(crate) fn ivfflat_membership_list_prefix(
    collection: &str,
    field: &str,
    list: usize,
) -> Vec<u8> {
    let list = u64::try_from(list).unwrap_or(u64::MAX).to_be_bytes();
    data_scoped_prefix(
        FAMILY_VECTOR_INDEX_STATE,
        collection,
        &[field.as_bytes(), b"l", &list],
    )
}

pub(crate) fn decode_ivfflat_membership_suffix(
    key: &[u8],
    prefix: &[u8],
) -> Option<(usize, String)> {
    let suffix = key.strip_prefix(prefix)?;
    let list = u64::from_be_bytes(suffix.get(..8)?.try_into().ok()?);
    if suffix.get(8).copied()? != LexKey::SEPARATOR {
        return None;
    }
    let id = std::str::from_utf8(suffix.get(9..)?).ok()?.to_string();
    Some((usize::try_from(list).ok()?, id))
}
