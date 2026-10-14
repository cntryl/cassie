//! PostgreSQL 18 catalog information behavior tracked by #817.

use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[test]
fn should_list_created_relations_through_pg_tables() {
    // Arrange
    let fixture = sql_fixture(
        "issue-817-pg-tables-created-relation",
        &[
            "CREATE SCHEMA issue_817_tables",
            "CREATE TABLE issue_817_tables.visible (id INTEGER)",
        ],
    );
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT schemaname, tablename FROM pg_catalog.pg_tables \
             WHERE schemaname = 'issue_817_tables' AND tablename = 'visible'",
        )
        .expect("query pg_tables for the created relation");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![
            Value::String("issue_817_tables".to_owned()),
            Value::String("visible".to_owned()),
        ]]
    );
}

#[test]
fn should_report_exact_postgresql_value_sizes() {
    // Arrange
    let fixture = sql_fixture("issue-817-column-size-values", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT pg_catalog.pg_column_size(CAST(0 AS INTEGER)), \
                    pg_catalog.pg_column_size(CAST(0 AS BIGINT)), \
                    pg_catalog.pg_column_size(CAST('foo' AS TEXT))",
        )
        .expect("query PostgreSQL datum sizes");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![Value::Int64(4), Value::Int64(8), Value::Int64(7)]]
    );
}

#[test]
fn should_resolve_serial_sequence_from_relation_column() {
    // Arrange
    let fixture = sql_fixture(
        "issue-817-get-serial-sequence",
        &[
            "CREATE SCHEMA issue_817_serial",
            "CREATE TABLE issue_817_serial.items (id SERIAL PRIMARY KEY)",
        ],
    );
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT pg_catalog.pg_get_serial_sequence( \
                'issue_817_serial.items', 'id')",
        )
        .expect("resolve the owned serial sequence");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![Value::String(
            "issue_817_serial.items_id_seq".to_owned()
        )]]
    );
}

#[test]
fn should_format_integer_byte_sizes_using_postgresql_units() {
    // Arrange
    let fixture = sql_fixture("issue-817-size-pretty", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT pg_catalog.pg_size_pretty(CAST(1024 AS BIGINT))")
        .expect("format the byte count");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![Value::String("1024 bytes".to_owned())]]
    );
}

#[test]
fn should_report_column_generation_metadata_in_information_schema() {
    // Arrange
    let fixture = sql_fixture(
        "issue-817-information-schema-column-generation",
        &["CREATE TABLE issue_817_plain_column (id INTEGER)"],
    );
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT column_name, is_identity, is_generated \
             FROM information_schema.columns \
             WHERE table_schema = 'public' \
               AND table_name = 'issue_817_plain_column' \
               AND column_name = 'id'",
        )
        .expect("read identity and generation metadata");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![
            Value::String("id".to_owned()),
            Value::String("NO".to_owned()),
            Value::String("NEVER".to_owned()),
        ]]
    );
}

#[test]
fn should_list_the_default_collation_in_pg_collation() {
    // Arrange
    let fixture = sql_fixture("issue-817-default-collation", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT collname, collprovider FROM pg_catalog.pg_collation \
             WHERE collname = 'default'",
        )
        .expect("read the default PostgreSQL collation");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![
            Value::String("default".to_owned()),
            Value::String("d".to_owned()),
        ]]
    );
}

#[test]
fn should_list_utc_in_pg_timezone_names() {
    // Arrange
    let fixture = sql_fixture("issue-817-utc-timezone-name", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT name, abbrev, utc_offset, is_dst \
             FROM pg_catalog.pg_timezone_names WHERE name = 'UTC'",
        )
        .expect("read the UTC timezone entry");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![
            Value::String("UTC".to_owned()),
            Value::String("UTC".to_owned()),
            Value::String("00:00:00".to_owned()),
            Value::Bool(false),
        ]]
    );
}

#[test]
fn should_render_a_created_view_definition() {
    // Arrange
    let fixture = sql_fixture(
        "issue-817-view-definition",
        &["CREATE VIEW issue_817_definition_view AS SELECT 42 AS answer"],
    );
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT pg_catalog.pg_get_viewdef('issue_817_definition_view') LIKE '%42%'")
        .expect("read the created view definition");

    // Assert
    assert_eq!(selected.rows(), vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_report_sequence_options_in_pg_sequences() {
    // Arrange
    let fixture = sql_fixture(
        "issue-817-sequence-catalog-options",
        &["CREATE SEQUENCE issue_817_catalog_seq"],
    );
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT schemaname, sequencename, start_value, increment_by, cycle \
             FROM pg_catalog.pg_sequences \
             WHERE schemaname = 'public' \
               AND sequencename = 'issue_817_catalog_seq'",
        )
        .expect("read sequence options from pg_sequences");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![
            Value::String("public".to_owned()),
            Value::String("issue_817_catalog_seq".to_owned()),
            Value::Int64(1),
            Value::Int64(1),
            Value::Bool(false),
        ]]
    );
}

#[test]
fn should_list_created_views_in_pg_views() {
    // Arrange
    let fixture = sql_fixture(
        "issue-817-pg-views-created-view",
        &["CREATE VIEW issue_817_catalog_view AS SELECT 42 AS answer"],
    );
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT schemaname, viewname FROM pg_catalog.pg_views \
             WHERE schemaname = 'public' AND viewname = 'issue_817_catalog_view'",
        )
        .expect("query pg_views for the created view");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![
            Value::String("public".to_owned()),
            Value::String("issue_817_catalog_view".to_owned()),
        ]]
    );
}

#[test]
fn should_expose_postgresql_object_descriptions() {
    // Arrange
    let fixture = sql_fixture("issue-817-pg-description", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT count(*) > 0 FROM pg_catalog.pg_description")
        .expect("query PostgreSQL object descriptions");

    // Assert
    assert_eq!(selected.rows(), vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_report_no_user_triggers_for_a_plain_table() {
    // Arrange
    let fixture = sql_fixture(
        "issue-817-pg-trigger-empty-table",
        &["CREATE TABLE issue_817_plain_trigger_table (id INTEGER)"],
    );
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT count(*) \
             FROM pg_catalog.pg_trigger AS trigger \
             JOIN pg_catalog.pg_class AS relation ON relation.oid = trigger.tgrelid \
             WHERE relation.relname = 'issue_817_plain_trigger_table'",
        )
        .expect("query triggers for the plain table");

    // Assert
    assert_eq!(selected.rows(), vec![vec![Value::Int64(0)]]);
}

#[test]
fn should_report_no_policies_for_a_plain_table() {
    // Arrange
    let fixture = sql_fixture(
        "issue-817-pg-policy-empty-table",
        &["CREATE TABLE issue_817_plain_policy_table (id INTEGER)"],
    );
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT count(*) \
             FROM pg_catalog.pg_policy AS policy \
             JOIN pg_catalog.pg_class AS relation ON relation.oid = policy.polrelid \
             WHERE relation.relname = 'issue_817_plain_policy_table'",
        )
        .expect("query row security policies for the plain table");

    // Assert
    assert_eq!(selected.rows(), vec![vec![Value::Int64(0)]]);
}

#[test]
fn should_list_accessed_relations_in_pg_stat_user_tables() {
    // Arrange
    let fixture = sql_fixture(
        "issue-817-pg-stat-user-tables",
        &["CREATE TABLE issue_817_stat_table (id INTEGER)"],
    );
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT relname FROM pg_catalog.pg_stat_user_tables \
             WHERE schemaname = 'public' AND relname = 'issue_817_stat_table'",
        )
        .expect("query the user table statistics view");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![Value::String("issue_817_stat_table".to_owned())]]
    );
}

#[test]
fn should_list_visible_builtin_routines_in_information_schema() {
    // Arrange
    let fixture = sql_fixture("issue-817-information-schema-routines", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT routine_schema, routine_name, routine_type \
             FROM information_schema.routines \
             WHERE routine_schema = 'pg_catalog' AND routine_name = 'now'",
        )
        .expect("query built-in routine metadata");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![
            Value::String("pg_catalog".to_owned()),
            Value::String("now".to_owned()),
            Value::String("FUNCTION".to_owned()),
        ]]
    );
}
