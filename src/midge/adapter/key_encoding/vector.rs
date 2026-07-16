use cntryl_lexkey::LexKey;

use super::{
    hnsw_source_summary_key, vector_hot_key, vector_hot_prefix, FAMILY_VECTOR_INDEX_STATE,
};

pub(crate) fn ivfflat_source_summary_key(relation_id: u64, field_id: u32) -> Vec<u8> {
    hnsw_source_summary_key(relation_id, field_id)
}

pub(crate) fn ivfflat_membership_key(
    relation_id: u64,
    field_id: u32,
    list: usize,
    id: &str,
) -> Vec<u8> {
    let list = u64::try_from(list).unwrap_or(u64::MAX).to_be_bytes();
    vector_hot_key(
        FAMILY_VECTOR_INDEX_STATE,
        relation_id,
        field_id,
        &[b"l", &list, id.as_bytes()],
    )
}

pub(crate) fn ivfflat_membership_prefix(relation_id: u64, field_id: u32) -> Vec<u8> {
    vector_hot_prefix(FAMILY_VECTOR_INDEX_STATE, relation_id, field_id, &[b"l"])
}

pub(crate) fn ivfflat_membership_list_prefix(
    relation_id: u64,
    field_id: u32,
    list: usize,
) -> Vec<u8> {
    let list = u64::try_from(list).unwrap_or(u64::MAX).to_be_bytes();
    vector_hot_prefix(
        FAMILY_VECTOR_INDEX_STATE,
        relation_id,
        field_id,
        &[b"l", &list],
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
