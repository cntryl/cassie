use super::{
    append_scalar_value, data_scoped_key, data_scoped_prefix, encoded_u64_component, key, prefix,
    CassieError, LexKey, FAMILY_COLUMN_BATCH, FAMILY_SCALAR_INDEX, FAMILY_UNIQUE_RESERVATION,
};

pub(crate) fn scalar_index_collection_prefix(relation_id: u64) -> Vec<u8> {
    let relation = encoded_u64_component(relation_id);
    prefix(FAMILY_SCALAR_INDEX, &[relation.as_slice()])
}

pub(crate) fn scalar_index_data_prefix(relation_id: u64, index_id: u64) -> Vec<u8> {
    numeric_index_prefix(FAMILY_SCALAR_INDEX, relation_id, index_id)
}

pub(crate) fn unique_constraint_reservation_prefix(collection: &str) -> Vec<u8> {
    data_scoped_prefix(FAMILY_UNIQUE_RESERVATION, collection, &[b"c"])
}

pub(crate) fn unique_constraint_reservation_field_prefix(collection: &str, field: &str) -> Vec<u8> {
    data_scoped_prefix(
        FAMILY_UNIQUE_RESERVATION,
        collection,
        &[b"c", field.as_bytes()],
    )
}

pub(crate) fn unique_constraint_reservation_key(
    collection: &str,
    field: &str,
    value: &serde_json::Value,
) -> Result<Vec<u8>, CassieError> {
    let mut key = data_scoped_key(
        FAMILY_UNIQUE_RESERVATION,
        collection,
        &[b"c", field.as_bytes()],
    );
    append_scalar_value(&mut key, value)?;
    Ok(key)
}

pub(crate) fn unique_scalar_index_reservation_key(
    collection: &str,
    index_name: &str,
    values: &[serde_json::Value],
) -> Result<Vec<u8>, CassieError> {
    let mut key = data_scoped_key(
        FAMILY_UNIQUE_RESERVATION,
        collection,
        &[b"i", index_name.as_bytes()],
    );
    for value in values {
        append_scalar_value(&mut key, value)?;
    }
    Ok(key)
}

pub(crate) fn unique_index_reservation_prefix(collection: &str) -> Vec<u8> {
    data_scoped_prefix(FAMILY_UNIQUE_RESERVATION, collection, &[b"i"])
}

pub(crate) fn unique_scalar_index_reservation_prefix(
    collection: &str,
    index_name: &str,
) -> Vec<u8> {
    data_scoped_prefix(
        FAMILY_UNIQUE_RESERVATION,
        collection,
        &[b"i", index_name.as_bytes()],
    )
}

pub(crate) fn column_batch_metadata_key(relation_id: u64, index_id: u64) -> Vec<u8> {
    let relation = encoded_u64_component(relation_id);
    let index = encoded_u64_component(index_id);
    key(
        FAMILY_COLUMN_BATCH,
        &[relation.as_slice(), index.as_slice(), b"m"],
    )
}

pub(crate) fn column_batch_segment_key(
    relation_id: u64,
    index_id: u64,
    segment_id: u64,
) -> Vec<u8> {
    let encoded_segment = LexKey::encode_u64(segment_id);
    let relation = encoded_u64_component(relation_id);
    let index = encoded_u64_component(index_id);
    key(
        FAMILY_COLUMN_BATCH,
        &[
            relation.as_slice(),
            index.as_slice(),
            b"s",
            encoded_segment.as_bytes(),
        ],
    )
}

pub(crate) fn column_batch_index_prefix(relation_id: u64, index_id: u64) -> Vec<u8> {
    numeric_index_prefix(FAMILY_COLUMN_BATCH, relation_id, index_id)
}

pub(crate) fn column_batch_collection_prefix(relation_id: u64) -> Vec<u8> {
    let relation = encoded_u64_component(relation_id);
    prefix(FAMILY_COLUMN_BATCH, &[relation.as_slice()])
}

fn numeric_index_prefix(family: &[u8], relation_id: u64, index_id: u64) -> Vec<u8> {
    let relation = encoded_u64_component(relation_id);
    let index = encoded_u64_component(index_id);
    prefix(family, &[relation.as_slice(), index.as_slice()])
}
