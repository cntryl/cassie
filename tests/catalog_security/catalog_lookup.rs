//! Direct catalog and system-information lookups tracked by #817.

use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[test]
fn should_map_postgresql_character_encoding_names() {
    // Arrange
    let fixture = sql_fixture("issue-817-character-encoding-ids", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT pg_catalog.pg_char_to_encoding('UTF8'), \
                    pg_catalog.pg_char_to_encoding('SQL_ASCII'), \
                    pg_catalog.pg_char_to_encoding('not-an-encoding')",
        )
        .expect("resolve PostgreSQL character encoding identifiers");

    // Assert
    assert_eq!(
        selected
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 23, 23]
    );
    assert_eq!(
        selected.rows(),
        vec![vec![Value::Int64(6), Value::Int64(0), Value::Int64(-1)]]
    );
}

#[test]
fn should_describe_reserved_keywords_from_the_server_keyword_table() {
    // Arrange
    let fixture = sql_fixture("issue-817-keyword-catalog-row", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT word, catcode, barelabel \
             FROM pg_catalog.pg_get_keywords() \
             WHERE word = 'select'",
        )
        .expect("read the server's SELECT keyword metadata");

    // Assert
    assert_eq!(
        selected
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![25, 18, 16]
    );
    assert_eq!(
        selected.rows(),
        vec![vec![
            Value::String("select".to_owned()),
            Value::String("R".to_owned()),
            Value::Bool(false),
        ]]
    );
}

#[test]
fn should_report_the_single_node_recovery_state() {
    // Arrange
    let fixture = sql_fixture("issue-817-recovery-state", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT pg_catalog.pg_is_in_recovery()")
        .expect("read recovery state");

    // Assert
    assert_eq!(selected.rows(), vec![vec![Value::Bool(false)]]);
}

#[test]
fn should_return_zero_when_the_session_has_no_temporary_schema() {
    // Arrange
    let fixture = sql_fixture("issue-817-no-temporary-schema", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT pg_catalog.pg_my_temp_schema()")
        .expect("read the current session temporary schema OID");

    // Assert
    assert_eq!(selected.rows(), vec![vec![Value::Int64(0)]]);
}

#[test]
fn should_report_zero_is_not_another_session_temporary_schema() {
    // Arrange
    let fixture = sql_fixture("issue-817-other-temporary-schema", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT pg_catalog.pg_is_other_temp_schema(0)")
        .expect("check whether the schema belongs to another session");

    // Assert
    assert_eq!(selected.rows(), vec![vec![Value::Bool(false)]]);
}

#[test]
fn should_parse_postgresql_size_strings_into_bytes() {
    // Arrange
    let fixture = sql_fixture("issue-817-size-bytes", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT pg_catalog.pg_size_bytes('1 kB'), pg_catalog.pg_size_bytes('1.5 MB')")
        .expect("parse PostgreSQL size strings");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![Value::Int64(1_024), Value::Int64(1_572_864)]]
    );
}

#[test]
fn should_report_postmaster_start_time_as_available() {
    // Arrange
    let fixture = sql_fixture("issue-817-postmaster-start-time", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT pg_catalog.pg_postmaster_start_time() IS NOT NULL")
        .expect("read the server start time");

    // Assert
    assert_eq!(selected.rows(), vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_report_select_privilege_for_the_pg_class_relation_name_column() {
    // Arrange
    let fixture = sql_fixture("issue-817-column-privilege", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT pg_catalog.has_column_privilege( \
                'pg_catalog.pg_class', 'relname', 'SELECT')",
        )
        .expect("check catalog column privilege");

    // Assert
    assert_eq!(selected.rows(), vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_report_execute_privilege_for_a_builtin_function() {
    // Arrange
    let fixture = sql_fixture("issue-817-function-privilege", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT pg_catalog.has_function_privilege( \
                'pg_catalog.abs(integer)', 'EXECUTE')",
        )
        .expect("check built-in function privilege");

    // Assert
    assert_eq!(selected.rows(), vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_report_usage_privilege_for_a_created_sequence() {
    // Arrange
    let fixture = sql_fixture(
        "issue-817-sequence-privilege",
        &["CREATE SEQUENCE issue_817_privilege_sequence"],
    );
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT pg_catalog.has_sequence_privilege( \
                'issue_817_privilege_sequence', 'USAGE')",
        )
        .expect("check the creating session's sequence privilege");

    // Assert
    assert_eq!(selected.rows(), vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_report_positive_size_for_the_active_database() {
    // Arrange
    let fixture = sql_fixture("issue-817-database-size", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT pg_catalog.pg_database_size(current_database()) > 0")
        .expect("read the active database size");

    // Assert
    assert_eq!(selected.rows(), vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_return_null_for_an_uncompressed_text_value() {
    // Arrange
    let fixture = sql_fixture("issue-817-column-compression", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT pg_catalog.pg_column_compression(CAST('plain' AS text)) IS NULL")
        .expect("read the compression method for plain text");

    // Assert
    assert_eq!(selected.rows(), vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_report_a_positive_current_transaction_id() {
    // Arrange
    let fixture = sql_fixture("issue-817-current-transaction-id", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT pg_catalog.txid_current() > 0")
        .expect("read the current transaction ID");

    // Assert
    assert_eq!(selected.rows(), vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_report_a_loaded_configuration_timestamp() {
    // Arrange
    let fixture = sql_fixture("issue-817-configuration-load-time", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT pg_catalog.pg_conf_load_time() IS NOT NULL")
        .expect("read the configuration load timestamp");

    // Assert
    assert_eq!(selected.rows(), vec![vec![Value::Bool(true)]]);
}
