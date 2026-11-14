//! Reads of a corrupt column table fail with `XX001`, name the relation and
//! row, return no partial result, and are reported by the verifier.

use super::{env, Env};
use cassie::app::CassieError;
use cassie::midge::adapter::StorageFamily;

const TABLE: &str = "victims";
const ID_FIELD: u32 = 1;
const NAME_FIELD: u32 = 2;

fn seeded() -> Env {
    let env = env();
    env.run("CREATE TABLE victims (id INT, name TEXT) WITH (storage = column_store)");
    for (row, id, name) in [("row-a", 1, "a"), ("row-b", 2, "b"), ("row-c", 3, "c")] {
        env.cassie
            .midge
            .put_document(
                TABLE,
                Some(row.to_string()),
                serde_json::json!({"id": id, "name": name}),
            )
            .expect("seed row");
    }
    env
}

fn assert_corruption(error: &CassieError, row: &str) {
    assert_eq!(error.sql_state(), "XX001", "{error}");
    let message = error.to_string();
    assert!(message.contains(TABLE), "{message}");
    assert!(message.contains(row), "{message}");
}

fn raw_delete(env: &Env, key: &[u8]) {
    env.cassie
        .midge
        .raw_delete(StorageFamily::Data, key)
        .expect("raw delete");
}

fn raw_put(env: &Env, key: &[u8], value: &[u8]) {
    env.cassie
        .midge
        .raw_put(StorageFamily::Data, key, value)
        .expect("raw put");
}

#[test]
fn should_fail_reads_when_a_live_marker_lacks_a_field_key() {
    // Arrange
    let env = seeded();
    raw_delete(&env, &env.field_key(TABLE, NAME_FIELD, "row-b"));

    // Act
    let point = env.cassie.midge.get_document(TABLE, "row-b");
    let scan = env
        .cassie
        .execute_sql(&env.session, "SELECT id, name FROM victims", vec![]);
    let verifier = env
        .cassie
        .midge
        .verify_column_store_for_diagnostics(TABLE)
        .expect("verifier");

    // Assert
    assert_corruption(&point.expect_err("point read fails"), "row-b");
    assert_corruption(&scan.expect_err("scan fails without partial rows"), "row-b");
    assert_eq!(verifier.len(), 1, "{verifier:?}");
    assert!(verifier[0].contains("row-b"));
}

#[test]
fn should_fail_reads_when_a_field_key_has_no_live_marker() {
    // Arrange
    let env = seeded();
    let valid_text = cassie_encoded_text("ghost");
    raw_put(
        &env,
        &env.field_key(TABLE, NAME_FIELD, "ghost"),
        &valid_text,
    );

    // Act
    let point = env.cassie.midge.get_document(TABLE, "ghost");
    let verifier = env
        .cassie
        .midge
        .verify_column_store_for_diagnostics(TABLE)
        .expect("verifier");

    // Assert
    assert_corruption(&point.expect_err("orphan read fails"), "ghost");
    assert_eq!(verifier.len(), 1, "{verifier:?}");
    assert!(verifier[0].contains("ghost"));
}

#[test]
fn should_fail_reads_when_a_value_does_not_decode() {
    // Arrange
    let env = seeded();
    raw_put(&env, &env.field_key(TABLE, NAME_FIELD, "row-c"), &[u8::MAX]);

    // Act
    let point = env.cassie.midge.get_document(TABLE, "row-c");
    let scan = env
        .cassie
        .execute_sql(&env.session, "SELECT name FROM victims", vec![]);

    // Assert
    assert_corruption(&point.expect_err("point read fails"), "row-c");
    assert_corruption(&scan.expect_err("scan fails"), "row-c");
}

#[test]
fn should_fail_reads_when_a_value_has_the_wrong_type() {
    // Arrange
    let env = seeded();
    // A text-tagged value where the INT column stores an integer.
    raw_put(
        &env,
        &env.field_key(TABLE, ID_FIELD, "row-a"),
        &cassie_encoded_text("x"),
    );

    // Act
    let point = env.cassie.midge.get_document(TABLE, "row-a");
    let verifier = env
        .cassie
        .midge
        .verify_column_store_for_diagnostics(TABLE)
        .expect("verifier");

    // Assert
    assert_corruption(&point.expect_err("type mismatch fails"), "row-a");
    assert_eq!(verifier.len(), 1, "{verifier:?}");
}

#[test]
fn should_fail_reads_when_a_live_marker_is_malformed() {
    // Arrange
    let env = seeded();
    raw_put(&env, &env.marker_key(TABLE, "row-a"), b"1");

    // Act
    let point = env.cassie.midge.get_document(TABLE, "row-a");
    let scan = env
        .cassie
        .execute_sql(&env.session, "SELECT id FROM victims", vec![]);

    // Assert
    assert_corruption(&point.expect_err("malformed marker fails"), "row-a");
    assert_corruption(&scan.expect_err("scan fails"), "row-a");
}

#[test]
fn should_resolve_a_field_added_after_the_row_was_written() {
    // Arrange
    let env = seeded();

    // Act
    env.cassie
        .midge
        .alter_collection_add_column(
            TABLE,
            cassie::types::FieldSchema {
                name: "later".to_string(),
                data_type: cassie::types::DataType::Text,
                nullable: true,
            },
        )
        .expect("add column");
    let old = env.cassie.midge.get_document(TABLE, "row-a");
    let verifier = env
        .cassie
        .midge
        .verify_column_store_for_diagnostics(TABLE)
        .expect("verifier");

    // Assert
    assert!(old.expect("old row still reads").is_some());
    assert!(verifier.is_empty(), "{verifier:?}");
}

#[test]
fn should_report_row_blob_keys_owned_by_a_column_table() {
    // Arrange
    let env = seeded();
    let row_prefix = env
        .cassie
        .midge
        .row_store_prefixes_for_diagnostics(TABLE)
        .expect("prefixes")
        .remove(0);
    let mut key = row_prefix;
    key.extend_from_slice(b"intruder");
    raw_put(&env, &key, b"CRB1");

    // Act
    let verifier = env
        .cassie
        .midge
        .verify_column_store_for_diagnostics(TABLE)
        .expect("verifier");

    // Assert
    assert_eq!(verifier.len(), 1, "{verifier:?}");
    assert!(verifier[0].contains("row blob"));
}

/// The compact encoding of a text value: the string tag followed by its bytes.
fn cassie_encoded_text(text: &str) -> Vec<u8> {
    let mut bytes = vec![0x05];
    bytes.extend_from_slice(text.as_bytes());
    bytes
}

#[test]
fn should_fail_a_full_scan_when_a_field_has_no_live_marker() {
    // Arrange
    let env = seeded();
    raw_put(
        &env,
        &env.field_key(TABLE, NAME_FIELD, "ghost"),
        &cassie_encoded_text("orphan"),
    );

    // Act
    let result = env
        .cassie
        .execute_sql(&env.session, "SELECT id, name FROM victims", vec![]);

    // Assert
    assert_corruption(
        &result.expect_err("orphan fields must fail the scan"),
        "ghost",
    );
}

#[test]
fn should_fail_a_batched_scan_when_a_field_has_no_live_marker() {
    // Arrange
    let env = seeded();
    raw_put(
        &env,
        &env.field_key(TABLE, NAME_FIELD, "ghost"),
        &cassie_encoded_text("orphan"),
    );

    // Act
    let result = env.cassie.midge.scan_documents_batched(TABLE, 2);

    // Assert
    assert_corruption(
        &result.expect_err("orphan fields must fail the scan"),
        "ghost",
    );
}

#[test]
fn should_preserve_orphan_fields_when_rejecting_conversion_to_heap() {
    // Arrange
    let env = seeded();
    let orphan_key = env.field_key(TABLE, NAME_FIELD, "ghost");
    let orphan_value = cassie_encoded_text("orphan");
    raw_put(&env, &orphan_key, &orphan_value);
    let entries = env.column_entries(TABLE);

    // Act
    let result = env.cassie.execute_sql(
        &env.session,
        "ALTER TABLE victims SET ACCESS METHOD heap",
        vec![],
    );
    let mode = env
        .rows("SELECT storage_mode FROM pg_catalog.pg_table_storage WHERE tablename = 'victims'");

    // Assert
    assert_corruption(
        &result.expect_err("conversion must reject orphan fields"),
        "ghost",
    );
    assert_eq!(
        mode,
        vec![vec![cassie::types::Value::String(
            "column-store".to_owned()
        )]]
    );
    assert_eq!(env.column_entries(TABLE), entries);
    assert!(env.row_store_keys(TABLE).is_empty());
}

#[test]
fn should_not_validate_unvisited_orphans_for_a_limit_one_query() {
    // Arrange
    let env = seeded();
    raw_put(
        &env,
        &env.field_key(TABLE, NAME_FIELD, "unvisited"),
        &cassie_encoded_text("orphan"),
    );
    let before = env.cassie.midge.column_validation_reads_for_diagnostics();

    // Act
    let rows = env.rows("SELECT id, name FROM victims LIMIT 1");

    // Assert
    assert_eq!(rows.len(), 1);
    assert_eq!(
        env.cassie.midge.column_validation_reads_for_diagnostics(),
        before
    );
    let full = env
        .cassie
        .execute_sql(&env.session, "SELECT id, name FROM victims", vec![]);
    assert_corruption(
        &full.expect_err("an unrestricted query still checks the full source"),
        "unvisited",
    );
    assert!(env.cassie.midge.column_validation_reads_for_diagnostics() > before);
}

#[test]
fn should_not_validate_unvisited_orphans_for_an_exists_probe() {
    // Arrange
    let env = seeded();
    raw_put(
        &env,
        &env.field_key(TABLE, NAME_FIELD, "unvisited"),
        &cassie_encoded_text("orphan"),
    );
    let before = env.cassie.midge.column_validation_reads_for_diagnostics();

    // Act
    let rows = env.rows("SELECT EXISTS (SELECT id FROM victims WHERE id = 1)");

    // Assert
    assert_eq!(rows, vec![vec![cassie::types::Value::Bool(true)]]);
    assert_eq!(
        env.cassie.midge.column_validation_reads_for_diagnostics(),
        before
    );
}
