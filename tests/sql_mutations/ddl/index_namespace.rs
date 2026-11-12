//! Index names share their schema's relation namespace.
use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[test]
fn should_bind_index_names_for_unscoped_and_two_part_collections() {
    // Arrange
    let fixture = sql_fixture("index-namespace-unscoped", &[]);
    let schema = cassie::types::Schema {
        fields: vec![cassie::types::FieldSchema {
            name: "v".to_owned(),
            data_type: cassie::types::DataType::Int,
            nullable: true,
        }],
    };
    fixture
        .cassie
        .register_collection("legacy_rows".to_owned(), schema.clone());
    fixture
        .cassie
        .register_collection("legacy_schema.rows".to_owned(), schema);

    // Act and assert
    for sql in [
        "CREATE INDEX legacy_idx ON legacy_rows (v)",
        "CREATE INDEX ON legacy_rows (v)",
        "CREATE INDEX legacy_idx ON legacy_schema.rows (v)",
        "CREATE INDEX ON legacy_schema.rows (v)",
    ] {
        let parsed = cassie::sql::parse_statement(sql).expect("parse");
        cassie::sql::binder::bind(parsed, &fixture.cassie.catalog).expect("bind legacy collection");
    }
}

#[test]
fn should_fail_closed_when_the_catalog_contains_an_unparseable_relation_name() {
    // Arrange
    let fixture = sql_fixture("index-namespace-malformed", &[]);
    let schema = cassie::types::Schema {
        fields: vec![cassie::types::FieldSchema {
            name: "v".to_owned(),
            data_type: cassie::types::DataType::Int,
            nullable: true,
        }],
    };
    fixture
        .cassie
        .register_collection("valid_rows".to_owned(), schema.clone());
    fixture
        .cassie
        .register_collection("invalid..name".to_owned(), schema);
    let parsed =
        cassie::sql::parse_statement("CREATE INDEX safe_name ON valid_rows (v)").expect("parse");

    // Act
    let error = cassie::sql::binder::bind(parsed, &fixture.cassie.catalog)
        .expect_err("malformed metadata must not be skipped");

    // Assert
    assert!(!error.to_string().is_empty());
}

#[test]
fn should_detect_same_schema_index_name_on_another_table() {
    // Arrange
    let fixture = sql_fixture(
        "index-namespace-collision",
        &[
            "CREATE SCHEMA idx_a",
            "CREATE SCHEMA idx_b",
            "CREATE TABLE idx_a.first_rows (v INT)",
            "CREATE TABLE idx_a.second_rows (v INT)",
            "CREATE TABLE idx_b.second_rows (v INT)",
            "CREATE INDEX dup_idx ON idx_a.first_rows (v)",
        ],
    );

    // Act
    let duplicate = fixture.execute("CREATE INDEX dup_idx ON idx_a.second_rows (v)");
    let skipped = fixture.execute("CREATE INDEX IF NOT EXISTS dup_idx ON idx_a.second_rows (v)");
    let invalid =
        fixture.execute("CREATE INDEX IF NOT EXISTS dup_idx ON idx_a.second_rows (missing)");
    let other_schema = fixture.execute("CREATE INDEX dup_idx ON idx_b.second_rows (v)");

    // Assert
    assert_eq!(
        duplicate
            .expect_err("schema name collision")
            .descriptor()
            .sql_state,
        "42P07"
    );
    skipped.expect("valid IF NOT EXISTS skips existing relation");
    assert_eq!(
        invalid
            .expect_err("column validation precedes skip")
            .descriptor()
            .sql_state,
        "42703"
    );
    other_schema.expect("other schemas have independent index names");
    assert!(fixture
        .cassie
        .catalog
        .get_index("idx_a.second_rows", "dup_idx")
        .is_none());
}

#[test]
fn should_reject_index_names_used_by_tables_and_sequences() {
    // Arrange
    let fixture = sql_fixture(
        "index-namespace-relations",
        &[
            "CREATE TABLE indexed_rows (v INT)",
            "CREATE TABLE occupied_name (v INT)",
            "CREATE SEQUENCE occupied_sequence",
        ],
    );

    // Act
    let table = fixture.execute("CREATE INDEX occupied_name ON indexed_rows (v)");
    let sequence = fixture.execute("CREATE INDEX occupied_sequence ON indexed_rows (v)");
    let skipped = fixture.execute("CREATE INDEX IF NOT EXISTS occupied_name ON indexed_rows (v)");

    // Assert
    assert_eq!(
        table
            .expect_err("table shares relation namespace")
            .descriptor()
            .sql_state,
        "42P07"
    );
    assert_eq!(
        sequence
            .expect_err("sequence shares relation namespace")
            .descriptor()
            .sql_state,
        "42P07"
    );
    skipped.expect("IF NOT EXISTS skips occupied relation name");
}

#[test]
fn should_reject_index_names_used_by_views() {
    // Arrange
    let fixture = sql_fixture(
        "index-namespace-views",
        &[
            "CREATE TABLE indexed_rows (v INT)",
            "CREATE VIEW occupied_view AS SELECT v FROM indexed_rows",
            "CREATE MATERIALIZED VIEW occupied_mv AS SELECT v FROM indexed_rows",
        ],
    );

    // Act
    let view = fixture.execute("CREATE INDEX occupied_view ON indexed_rows (v)");
    let materialized = fixture.execute("CREATE INDEX occupied_mv ON indexed_rows (v)");

    // Assert
    assert_eq!(
        view.expect_err("view owns relation name")
            .descriptor()
            .sql_state,
        "42P07"
    );
    assert_eq!(
        materialized
            .expect_err("materialized view owns relation name")
            .descriptor()
            .sql_state,
        "42P07"
    );
}

#[test]
fn should_reject_case_distinct_index_names_until_identity_is_safe() {
    // Arrange
    let fixture = sql_fixture(
        "index-namespace-quoted",
        &["CREATE TABLE indexed_rows (v INT)"],
    );
    fixture
        .execute("CREATE INDEX \"Case.Index\" ON indexed_rows (v)")
        .unwrap();

    // Act
    let ambiguous = fixture.execute("CREATE INDEX \"case.index\" ON indexed_rows (v)");

    // Assert
    assert_eq!(
        ambiguous
            .expect_err("case-distinct identity is not yet safe")
            .descriptor()
            .sql_state,
        "0A000"
    );
    let first_oid = fixture.rows("SELECT oid::bigint FROM pg_class WHERE relname = 'Case.Index'");
    fixture
        .execute("ALTER INDEX \"Case.Index\" RENAME TO \"Other.Index\"")
        .expect("rename exact quoted name");
    assert_eq!(
        fixture.rows("SELECT indexname FROM pg_indexes WHERE tablename = 'indexed_rows'"),
        vec![vec![Value::String("Other.Index".to_owned())]]
    );
    assert_eq!(
        fixture.rows("SELECT oid::bigint FROM pg_class WHERE relname = 'Other.Index'"),
        first_oid
    );
    fixture
        .execute("DROP INDEX \"Other.Index\"")
        .expect("drop exact quoted name");
}

#[test]
fn should_preserve_unquoted_lookup_for_an_existing_mixed_case_index() {
    // Arrange
    let fixture = sql_fixture(
        "index-namespace-existing-case",
        &["CREATE TABLE indexed_rows (v INT)"],
    );
    fixture
        .execute("CREATE INDEX \"MixedCase\" ON indexed_rows (v)")
        .expect("create mixed-case index");

    // Act
    fixture
        .execute("DROP INDEX mixedcase")
        .expect("existing unquoted case-folded lookup");

    // Assert
    assert!(fixture
        .rows("SELECT indexname FROM pg_indexes WHERE tablename = 'indexed_rows'")
        .is_empty());
}

#[test]
fn should_validate_include_and_predicate_before_skipping_an_existing_name() {
    // Arrange
    let fixture = sql_fixture(
        "index-namespace-if-not-exists-validation",
        &[
            "CREATE TABLE indexed_rows (v INT)",
            "CREATE INDEX existing_idx ON indexed_rows (v)",
        ],
    );

    // Act
    let include = fixture
        .execute("CREATE INDEX IF NOT EXISTS existing_idx ON indexed_rows (v) INCLUDE (missing)");
    let predicate = fixture
        .execute("CREATE INDEX IF NOT EXISTS existing_idx ON indexed_rows (v) WHERE missing = 1");

    // Assert
    assert_eq!(
        include
            .expect_err("missing INCLUDE column")
            .descriptor()
            .sql_state,
        "42703"
    );
    assert_eq!(
        predicate
            .expect_err("missing predicate column")
            .descriptor()
            .sql_state,
        "42703"
    );
}

#[test]
fn should_choose_generated_index_name_across_the_schema() {
    // Arrange
    let fixture = sql_fixture(
        "index-namespace-generated",
        &[
            "CREATE TABLE first_rows (v INT)",
            "CREATE TABLE generated_rows (v INT)",
            "CREATE INDEX generated_rows_v_idx ON first_rows (v)",
        ],
    );

    // Act
    fixture
        .execute("CREATE INDEX ON generated_rows (v)")
        .expect("generated index");

    // Assert
    assert_eq!(
        fixture.rows("SELECT indexname FROM pg_indexes WHERE tablename = 'generated_rows'"),
        vec![vec![Value::String("generated_rows_v_idx1".to_owned())]]
    );
}

#[test]
fn should_shorten_both_generated_name_components_before_the_index_suffix() {
    // Arrange
    let table = "a".repeat(40);
    let column = "b".repeat(40);
    let fixture = sql_fixture("index-namespace-long-generated", &[]);
    fixture
        .execute(&format!("CREATE TABLE {table} ({column} INT)"))
        .expect("create table");

    // Act
    fixture
        .execute(&format!("CREATE INDEX ON {table} ({column})"))
        .expect("create index");

    // Assert
    let expected = format!("{}_{}_idx", "a".repeat(29), "b".repeat(29));
    assert_eq!(
        fixture.rows(&format!(
            "SELECT indexname FROM pg_indexes WHERE tablename = '{table}'"
        )),
        vec![vec![Value::String(expected)]]
    );
}

#[test]
fn should_clip_generated_name_at_a_utf8_character_boundary() {
    // Arrange
    let table = "é".repeat(30);
    let column = "b".repeat(40);
    let fixture = sql_fixture("index-namespace-unicode-generated", &[]);
    fixture
        .execute(&format!("CREATE TABLE \"{table}\" ({column} INT)"))
        .expect("create table");

    // Act
    fixture
        .execute(&format!("CREATE INDEX ON \"{table}\" ({column})"))
        .expect("create index");

    // Assert
    let expected = format!("{}_{}_idx", "é".repeat(14), "b".repeat(29));
    assert_eq!(
        fixture.rows(&format!(
            "SELECT indexname FROM pg_indexes WHERE tablename = '{table}'"
        )),
        vec![vec![Value::String(expected)]]
    );
}

#[test]
fn should_restore_schema_namespace_after_savepoint_rollback() {
    // Arrange
    let fixture = sql_fixture(
        "index-namespace-savepoint",
        &[
            "CREATE TABLE first_rows (v INT)",
            "CREATE TABLE second_rows (v INT)",
            "BEGIN",
            "SAVEPOINT before_index",
            "CREATE INDEX shared_idx ON first_rows (v)",
        ],
    );

    // Act
    let collision = fixture.execute("CREATE INDEX shared_idx ON second_rows (v)");
    fixture
        .execute("ROLLBACK TO before_index")
        .expect("recover savepoint");
    fixture
        .execute("CREATE INDEX shared_idx ON second_rows (v)")
        .expect("name restored");
    fixture.execute("COMMIT").expect("commit");

    // Assert
    assert_eq!(
        collision
            .expect_err("staged index owns name")
            .descriptor()
            .sql_state,
        "42P07"
    );
    assert!(fixture
        .cassie
        .catalog
        .get_index("second_rows", "shared_idx")
        .is_some());
}

#[test]
fn should_reject_concurrent_catalog_publication_of_the_same_index_name() {
    // Arrange
    let fixture = sql_fixture(
        "index-namespace-concurrent",
        &[
            "CREATE TABLE namespace_left (v INT)",
            "CREATE TABLE namespace_right (v INT)",
        ],
    );
    let other = fixture.cassie.create_session("tester", None);
    fixture.execute("BEGIN").expect("begin first transaction");
    fixture
        .cassie
        .execute_sql(&other, "BEGIN", vec![])
        .expect("begin second transaction");
    fixture
        .execute("CREATE INDEX shared_idx ON namespace_left (v)")
        .expect("stage first index");
    fixture
        .cassie
        .execute_sql(
            &other,
            "CREATE INDEX shared_idx ON namespace_right (v)",
            vec![],
        )
        .expect("stage second index");

    // Act
    fixture.execute("COMMIT").expect("commit first transaction");
    let conflict = fixture
        .cassie
        .execute_sql(&other, "COMMIT", vec![])
        .expect_err("stale catalog publication fails");
    fixture
        .cassie
        .execute_sql(&other, "ROLLBACK", vec![])
        .expect("recover second transaction");

    // Assert
    assert_eq!(conflict.descriptor().sql_state, "40001");
    assert_eq!(fixture.rows("SELECT indexname FROM pg_indexes WHERE tablename IN ('namespace_left', 'namespace_right')"),
        vec![vec![Value::String("shared_idx".to_owned())]]);
}
