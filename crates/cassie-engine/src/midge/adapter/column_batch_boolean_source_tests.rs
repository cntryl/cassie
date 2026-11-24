//! Required Boolean chunks retain the complete-source validation and fallback boundary.
use crate::app::{Cassie, CassieSession};
use crate::catalog::{ColumnBatchMetadata, IndexMeta};
use crate::config::CassieRuntimeConfig;
use crate::midge::adapter::column_batch_format_v1::{self as format, Codec};
use crate::midge::adapter::ColumnBatchScanFallbackReason;
use crate::runtime::QueryExecutionControls;
use std::time::Instant;

fn fixture(
    codec: Option<Codec>,
) -> (
    Cassie,
    CassieSession,
    String,
    IndexMeta,
    ColumnBatchMetadata,
) {
    fixture_with_segments(codec, 1024)
}
fn fixture_with_segments(
    codec: Option<Codec>,
    segment_size: usize,
) -> (
    Cassie,
    CassieSession,
    String,
    IndexMeta,
    ColumnBatchMetadata,
) {
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE bool_source (n BIGINT,b BOOLEAN)",
            vec![],
        )
        .expect("schema");
    let flags = if codec == Some(Codec::Constant) {
        [true, true, true]
    } else {
        [false, true, false]
    };
    for (index, flag) in flags.into_iter().enumerate() {
        cassie
            .midge
            .put_document(
                "bool_source",
                Some(format!("row{index}")),
                serde_json::json!({"n":index,"b":flag}),
            )
            .expect("source row");
    }
    cassie
        .execute_sql(
            &session,
            &format!(
                "CREATE INDEX bool_cover ON bool_source USING column (n,b) WITH (segment_size={segment_size})"
            ),
            vec![],
        )
        .expect("index");
    let stored = cassie
        .midge
        .get_index("bool_source", "bool_cover")
        .expect("index read")
        .expect("index");
    let table = stored.collection.clone();
    let metadata = cassie
        .midge
        .get_column_batch_metadata(&table, &stored.name)
        .expect("manifest")
        .expect("manifest");
    (cassie, session, table, stored, metadata)
}
fn replace(
    cassie: &Cassie,
    index: &crate::catalog::IndexMeta,
    metadata: &ColumnBatchMetadata,
    raw: Vec<u8>,
    position: usize,
) {
    cassie
        .midge
        .replace_column_batch_chunk_for_test(index, metadata, "b", raw, position)
        .expect("replace fixture chunk");
}
fn controls(cassie: &Cassie) -> QueryExecutionControls {
    QueryExecutionControls::from_limits(&cassie.runtime.limits(), Instant::now())
}
fn diagnose(
    cassie: &Cassie,
    table: &str,
    fields: &[String],
    controls: &QueryExecutionControls,
) -> (
    bool,
    Option<ColumnBatchScanFallbackReason>,
    usize,
    Option<bool>,
    usize,
) {
    let request = controls.storage_request();
    cassie
        .midge
        .diagnose_controlled_encoded_source_for_test(table, fields, &request)
        .expect("controlled encoded source diagnostics")
}

#[test]
fn should_keep_nonplain_boolean_chunks_on_the_validated_json_path() {
    // Arrange
    for codec in [Codec::Constant, Codec::Rle, Codec::Dictionary] {
        let (cassie, _session, table, index, mut metadata) = fixture(Some(codec));
        let raw = format::boolean_tests::nonplain(codec);
        let chunk = metadata.segments[0]
            .field_chunks
            .get_mut("b")
            .expect("Boolean metadata");
        chunk.codec_id = codec as u8;
        chunk.codec_name = codec.name().into();
        chunk.encoded_len = raw.len();
        chunk.checksum_sha256 = format::checksum_hex(&raw);
        replace(&cassie, &index, &metadata, raw, 0);
        let controls = controls(&cassie);
        // Act
        let (hit, fallback, materialized_values, boolean_payload, value_count) =
            diagnose(&cassie, &table, &["b".into()], &controls);
        // Assert
        assert!(hit, "non-Plain Boolean remains supported");
        assert_eq!(fallback, None);
        assert_eq!(boolean_payload, Some(false));
        assert_eq!(value_count, 3);
        assert_eq!(materialized_values, 3);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_discard_the_complete_source_for_a_required_boolean_checksum_failure() {
    // Arrange
    let (cassie, _session, table, index, metadata) = fixture(None);
    let raw = vec![0_u8; metadata.segments[0].field_chunks["b"].encoded_len];
    replace(&cassie, &index, &metadata, raw, 0);
    let controls = controls(&cassie);
    // Act
    let (hit, fallback, _, _, _) =
        diagnose(&cassie, &table, &["n".into(), "b".into()], &controls);
    // Assert
    assert!(!hit);
    assert!(matches!(
        fallback,
        Some(ColumnBatchScanFallbackReason::SegmentChecksumMismatch)
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_discard_a_checksum_consistent_malformed_boolean_chunk() {
    // Arrange
    let (cassie, session, table, index, mut metadata) = fixture_with_segments(None, 1);
    let mut raw =
        format::encode_plain_column_chunk(format::LogicalType::Boolean, &[serde_json::json!(true)])
            .expect("Plain")
            .bytes;
    raw[34] |= 0b1000_0000;
    metadata.segments[1]
        .field_chunks
        .get_mut("b")
        .expect("Boolean metadata")
        .checksum_sha256 = format::checksum_hex(&raw);
    replace(&cassie, &index, &metadata, raw, 1);
    let controls = controls(&cassie);
    // Act
    let (hit, fallback, _, _, _) =
        diagnose(&cassie, &table, &["n".into(), "b".into()], &controls);
    // Assert
    assert!(!hit);
    assert!(matches!(
        fallback,
        Some(ColumnBatchScanFallbackReason::InvalidPayload)
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
    let public = cassie
        .execute_sql(
            &session,
            "SELECT SUM(n),COUNT(*) FROM bool_source WHERE b IS TRUE",
            vec![],
        )
        .expect("authoritative fallback");
    assert_eq!(
        public.rows(),
        &[vec![
            cassie_core::types::Value::Int64(1),
            cassie_core::types::Value::Int64(1)
        ]]
    );
}

#[test]
fn should_reject_boolean_chunk_metadata_disagreement_without_publishing_a_source() {
    // Arrange
    let (cassie, _session, table, index, mut metadata) = fixture(None);
    let raw = format::encode_plain_column_chunk(
        format::LogicalType::Boolean,
        &[
            serde_json::json!(false),
            serde_json::json!(true),
            serde_json::json!(false),
        ],
    )
    .expect("Plain")
    .bytes;
    metadata.segments[0]
        .field_chunks
        .get_mut("b")
        .expect("Boolean metadata")
        .null_count = 1;
    replace(&cassie, &index, &metadata, raw, 0);
    let controls = controls(&cassie);
    // Act
    let (hit, fallback, _, _, _) =
        diagnose(&cassie, &table, &["n".into(), "b".into()], &controls);
    // Assert
    assert!(!hit);
    assert!(fallback.is_some());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
