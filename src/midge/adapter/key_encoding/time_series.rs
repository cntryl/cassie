use super::{
    append_terminated_component, encoded_u64_component, key, prefix, LexKey,
    FAMILY_TIME_SERIES_INDEX,
};

const ARTIFACT_MANIFEST: [u8; 1] = [1];
const ARTIFACT_MEMBERSHIP: [u8; 1] = [2];
const ARTIFACT_BUCKET_COUNT: [u8; 1] = [3];

pub(crate) fn time_series_index_collection_prefix(relation_id: u64) -> Vec<u8> {
    let relation = encoded_u64_component(relation_id);
    prefix(FAMILY_TIME_SERIES_INDEX, &[relation.as_slice()])
}

pub(crate) fn time_series_index_artifact_prefix(relation_id: u64, index_id: u64) -> Vec<u8> {
    numeric_index_prefix(relation_id, index_id)
}

pub(crate) fn time_series_index_data_prefix(relation_id: u64, index_id: u64) -> Vec<u8> {
    let relation = encoded_u64_component(relation_id);
    let index = encoded_u64_component(index_id);
    prefix(
        FAMILY_TIME_SERIES_INDEX,
        &[
            relation.as_slice(),
            index.as_slice(),
            ARTIFACT_MEMBERSHIP.as_slice(),
        ],
    )
}

pub(crate) fn time_series_index_manifest_key(relation_id: u64, index_id: u64) -> Vec<u8> {
    let relation = encoded_u64_component(relation_id);
    let index = encoded_u64_component(index_id);
    key(
        FAMILY_TIME_SERIES_INDEX,
        &[
            relation.as_slice(),
            index.as_slice(),
            ARTIFACT_MANIFEST.as_slice(),
        ],
    )
}

pub(crate) fn time_series_index_bucket_count_prefix(relation_id: u64, index_id: u64) -> Vec<u8> {
    let relation = encoded_u64_component(relation_id);
    let index = encoded_u64_component(index_id);
    prefix(
        FAMILY_TIME_SERIES_INDEX,
        &[
            relation.as_slice(),
            index.as_slice(),
            ARTIFACT_BUCKET_COUNT.as_slice(),
        ],
    )
}

pub(crate) fn time_series_index_bucket_count_key(
    relation_id: u64,
    index_id: u64,
    partition_key: &str,
    bucket_start_seconds: i64,
) -> Vec<u8> {
    let mut key = time_series_index_bucket_count_prefix(relation_id, index_id);
    append_terminated_component(&mut key, partition_key.as_bytes());
    key.extend_from_slice(sortable_i64_hex(bucket_start_seconds).as_bytes());
    key
}

pub(crate) fn decode_time_series_bucket_count_key(
    key: &[u8],
    count_prefix: &[u8],
) -> Option<(String, i64)> {
    let components = decoded_components(key.strip_prefix(count_prefix)?)?;
    let [partition, bucket] = components.as_slice() else {
        return None;
    };
    Some(((*partition).to_string(), decode_sortable_i64_hex(bucket)?))
}

pub(crate) fn time_series_index_entry_key(
    relation_id: u64,
    index_id: u64,
    partition_key: &str,
    bucket_start_seconds: i64,
    timestamp_seconds: i64,
    timestamp_nanoseconds: u32,
    id: &str,
) -> Vec<u8> {
    let mut key = time_series_index_partition_prefix(relation_id, index_id, partition_key);
    key.extend_from_slice(sortable_i64_hex(bucket_start_seconds).as_bytes());
    key.push(LexKey::SEPARATOR);
    key.extend_from_slice(sortable_i64_hex(timestamp_seconds).as_bytes());
    key.push(LexKey::SEPARATOR);
    key.extend_from_slice(format!("{timestamp_nanoseconds:08x}").as_bytes());
    key.push(LexKey::SEPARATOR);
    append_terminated_component(&mut key, id.as_bytes());
    key
}

pub(crate) fn time_series_index_partition_prefix(
    relation_id: u64,
    index_id: u64,
    partition_key: &str,
) -> Vec<u8> {
    let mut key = time_series_index_data_prefix(relation_id, index_id);
    append_terminated_component(&mut key, partition_key.as_bytes());
    key
}

pub(crate) fn time_series_index_bucket_bound_key(
    relation_id: u64,
    index_id: u64,
    partition_key: &str,
    bucket_start_seconds: i64,
) -> Vec<u8> {
    let mut key = time_series_index_partition_prefix(relation_id, index_id, partition_key);
    key.extend_from_slice(sortable_i64_hex(bucket_start_seconds).as_bytes());
    key
}

pub(crate) fn decode_time_series_entry_key(
    key: &[u8],
    data_prefix: &[u8],
) -> Option<(String, i64, i64, u32, String)> {
    let components = decoded_components(key.strip_prefix(data_prefix)?)?;
    let [partition, bucket, timestamp, nanos, id] = components.as_slice() else {
        return None;
    };
    Some((
        (*partition).to_string(),
        decode_sortable_i64_hex(bucket)?,
        decode_sortable_i64_hex(timestamp)?,
        u32::from_str_radix(nanos, 16).ok()?,
        (*id).to_string(),
    ))
}

fn numeric_index_prefix(relation_id: u64, index_id: u64) -> Vec<u8> {
    let relation = encoded_u64_component(relation_id);
    let index = encoded_u64_component(index_id);
    prefix(
        FAMILY_TIME_SERIES_INDEX,
        &[relation.as_slice(), index.as_slice()],
    )
}

fn decoded_components(suffix: &[u8]) -> Option<Vec<&str>> {
    suffix
        .split(|byte| *byte == LexKey::SEPARATOR)
        .filter(|component| !component.is_empty())
        .map(std::str::from_utf8)
        .collect::<Result<Vec<_>, _>>()
        .ok()
}

fn sortable_i64_hex(value: i64) -> String {
    format!("{:016x}", value.cast_unsigned() ^ (1_u64 << 63))
}

fn decode_sortable_i64_hex(value: &str) -> Option<i64> {
    Some((u64::from_str_radix(value, 16).ok()? ^ (1_u64 << 63)).cast_signed())
}
