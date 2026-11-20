use cntryl_lexkey::LexKey;

use super::{
    encoded_u64_component, key, prefix, FAMILY_NORMALIZED_VECTOR, FAMILY_VECTOR_INDEX_STATE,
};

pub(crate) fn vector_index_state_key(relation_id: u64, field_id: u32) -> Vec<u8> {
    vector_hot_key(FAMILY_VECTOR_INDEX_STATE, relation_id, field_id, &[])
}

pub(crate) fn hnsw_graph_node_key(relation_id: u64, field_id: u32, id: &str) -> Vec<u8> {
    vector_hot_key(
        FAMILY_VECTOR_INDEX_STATE,
        relation_id,
        field_id,
        &[b"n", id.as_bytes()],
    )
}

pub(crate) fn hnsw_graph_node_prefix(relation_id: u64, field_id: u32) -> Vec<u8> {
    vector_hot_prefix(FAMILY_VECTOR_INDEX_STATE, relation_id, field_id, &[b"n"])
}

pub(crate) fn hnsw_source_summary_key(relation_id: u64, field_id: u32) -> Vec<u8> {
    vector_hot_key(FAMILY_VECTOR_INDEX_STATE, relation_id, field_id, &[b"f"])
}

/// Generation of the last full normalized-vector publication for one field.
///
/// The field component carries a trailing marker byte, so the key sits beside
/// the field's normalized vectors without falling inside their scan prefix, and
/// under the collection prefix that drops them together.
pub(crate) fn normalized_vector_floor_key(relation_id: u64, field_id: u32) -> Vec<u8> {
    let relation = encoded_u64_component(relation_id);
    let mut field = field_id.to_be_bytes().to_vec();
    field.push(b'g');
    key(
        FAMILY_NORMALIZED_VECTOR,
        &[relation.as_slice(), field.as_slice()],
    )
}

pub(crate) fn vector_index_state_prefix(relation_id: u64) -> Vec<u8> {
    let relation = encoded_u64_component(relation_id);
    prefix(FAMILY_VECTOR_INDEX_STATE, &[relation.as_slice()])
}

pub(crate) fn normalized_vector_key(relation_id: u64, field_id: u32, id: &str) -> Vec<u8> {
    vector_hot_key(
        FAMILY_NORMALIZED_VECTOR,
        relation_id,
        field_id,
        &[id.as_bytes()],
    )
}

pub(crate) fn normalized_vector_prefix(relation_id: u64, field_id: u32) -> Vec<u8> {
    vector_hot_prefix(FAMILY_NORMALIZED_VECTOR, relation_id, field_id, &[])
}

pub(crate) fn normalized_vector_collection_prefix(relation_id: u64) -> Vec<u8> {
    let relation = encoded_u64_component(relation_id);
    prefix(FAMILY_NORMALIZED_VECTOR, &[relation.as_slice()])
}

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

fn vector_hot_key(family: &[u8], relation_id: u64, field_id: u32, suffix: &[&[u8]]) -> Vec<u8> {
    let relation = encoded_u64_component(relation_id);
    let field = field_id.to_be_bytes();
    let mut components = vec![relation.as_slice(), field.as_slice()];
    components.extend_from_slice(suffix);
    key(family, &components)
}

fn vector_hot_prefix(family: &[u8], relation_id: u64, field_id: u32, suffix: &[&[u8]]) -> Vec<u8> {
    let mut value = vector_hot_key(family, relation_id, field_id, suffix);
    value.push(LexKey::SEPARATOR);
    value
}
