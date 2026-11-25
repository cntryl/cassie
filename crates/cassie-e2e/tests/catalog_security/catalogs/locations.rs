//! Midge location rejection and truthful absence of PostgreSQL TOAST chunks.

use crate::support_backend_sessions::assert_rows;
use crate::support_catalog_policy as policy;
use crate::support_pgwire as wire;
use std::fmt::Write as _;

#[test]
fn should_report_truthful_relation_locations_given_live_relations_when_storage_location_helpers_are_called(
) {
    // Arrange
    // Approved architecture policy: Midge tables have no PostgreSQL heap files.
    // Typed NULL calls still follow the strict PostgreSQL signatures.
    let statements = [
        "CREATE TABLE location_policy_rows (k integer)",
        "SELECT pg_relation_filenode(NULL::regclass), pg_relation_filepath(NULL::regclass), pg_filenode_relation(NULL::oid, NULL::oid)",
        "SELECT pg_relation_filenode('location_policy_rows'::regclass)",
        "SELECT pg_relation_filepath('location_policy_rows'::regclass)",
        "SELECT pg_filenode_relation(0::oid, 'location_policy_rows'::regclass::oid)",
        "SELECT 42",
    ];

    // Act
    let batches = policy::run("relation-location-policy", &statements);

    // Assert
    assert_rows(&batches[0], &[]);
    policy::assert_nulls(&batches[1], &[26, 25, 2205]);
    for frames in &batches[2..5] {
        policy::assert_error(frames, "0A000");
    }
    assert_rows(&batches[5], &[&["42"]]);
}

#[test]
fn should_follow_toast_identity_policy_given_stored_values_when_pg_column_toast_chunk_id_is_called()
{
    // Arrange
    // Midge has no PostgreSQL TOAST chunks, including for external-sized
    // payloads. A fabricated PostgreSQL chunk OID is not acceptable.
    let mut large_payload = String::new();
    for value in 0..4096 {
        write!(&mut large_payload, "{value:08x}").expect("append a payload fragment");
    }
    let insert = format!(
        "INSERT INTO toast_policy_rows VALUES \
         (1, 'inline'), (2, ''), (3, NULL), (4, '{large_payload}')"
    );
    let statements = [
        "CREATE TABLE toast_policy_rows (k integer, payload text)",
        &insert,
        "SELECT k, octet_length(payload), pg_column_toast_chunk_id(payload) \
         FROM toast_policy_rows ORDER BY k",
    ];

    // Act
    let batches = policy::run("column-toast-identity", &statements);

    // Assert
    assert_rows(&batches[0], &[]);
    assert_rows(&batches[1], &[]);
    assert_eq!(wire::error_code(&batches[2]), None);
    let (_, body) = batches[2]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("TOAST identity descriptor");
    assert_eq!(
        wire::parse_row_description(body)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 23, 26]
    );
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![
            vec![Some("1".to_owned()), Some("6".to_owned()), None],
            vec![Some("2".to_owned()), Some("0".to_owned()), None],
            vec![Some("3".to_owned()), None, None],
            vec![Some("4".to_owned()), Some("32768".to_owned()), None]
        ]
    );
}
