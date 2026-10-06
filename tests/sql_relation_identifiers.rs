#[path = "support/sql.rs"]
mod support_sql;
#[path = "support/sql_fixture.rs"]
mod support_sql_fixture;
#[path = "support/temp_dirs.rs"]
mod support_temp_dirs;

use support_sql_fixture::sql_fixture;

#[test]
fn should_preserve_quoted_dotted_relation_identity_after_restart() {
    // Arrange
    support_sql::use_local_storage();
    let path = support_sql::data_dir("quoted_relation_restart");
    let cassie = cassie::app::Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(&session, "CREATE SCHEMA triage", vec![])
        .expect("create schema");
    cassie
        .execute_sql(&session, "CREATE TABLE triage.dot (id BIGINT)", vec![])
        .expect("create schema-qualified relation");
    cassie
        .execute_sql(&session, "CREATE TABLE \"triage.dot\" (id BIGINT)", vec![])
        .expect("create quoted dotted relation");
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE \"triage.dot\"\"archive \" (id BIGINT)",
            vec![],
        )
        .expect("create relation with escaped quote and trailing space");
    cassie
        .execute_sql(&session, "INSERT INTO triage.dot (id) VALUES (9)", vec![])
        .expect("insert schema-qualified relation");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO \"triage.dot\" (id) VALUES (7)",
            vec![],
        )
        .expect("insert quoted dotted relation");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO \"triage.dot\"\"archive \" (id) VALUES (5)",
            vec![],
        )
        .expect("insert relation with escaped quote and trailing space");
    drop(session);
    drop(cassie);

    let restarted = cassie::app::Cassie::new_with_data_dir(&path).expect("reopen Cassie");
    restarted.startup().expect("start reopened Cassie");
    let session = restarted.create_session("tester", None);

    // Act
    let dotted_rows = restarted
        .execute_sql(&session, "SELECT id FROM \"triage.dot\"", vec![])
        .expect("read quoted dotted relation after restart");
    let qualified_rows = restarted
        .execute_sql(&session, "SELECT id FROM triage.dot", vec![])
        .expect("read schema-qualified relation after restart");
    let escaped_rows = restarted
        .execute_sql(
            &session,
            "SELECT id FROM \"triage.dot\"\"archive \"",
            vec![],
        )
        .expect("read relation with escaped quote and trailing space after restart");

    // Assert
    assert_eq!(dotted_rows.rows, vec![vec![cassie::types::Value::Int64(7)]]);
    assert_eq!(
        qualified_rows.rows,
        vec![vec![cassie::types::Value::Int64(9)]]
    );
    assert_eq!(
        escaped_rows.rows,
        vec![vec![cassie::types::Value::Int64(5)]]
    );
    drop(session);
    drop(restarted);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_copy_rows_into_a_quoted_dotted_relation() {
    // Arrange
    let fixture = relation_fixture("quoted_relation_copy", false);
    fixture
        .execute("CREATE TABLE \"triage.dot\" (id BIGINT)")
        .expect("create quoted dotted relation");
    let parsed =
        cassie::sql::parse_statement("COPY \"triage.dot\" (id) FROM STDIN WITH (FORMAT csv)")
            .expect("parse COPY statement");
    let cassie::sql::QueryStatement::Copy(statement) = parsed.statement else {
        panic!("COPY statement expected");
    };

    // Act
    let copied = fixture
        .cassie
        .copy_from_csv_stdin(&fixture.session, &statement, b"12\n");

    // Assert
    assert!(copied.is_ok(), "COPY should succeed");
    assert_eq!(
        fixture.rows("SELECT id FROM \"triage.dot\""),
        vec![vec![cassie::types::Value::Int64(12)]]
    );
}

#[test]
fn should_distinguish_quoted_dotted_relation_from_schema_qualified_relation() {
    // Arrange
    let fixture = relation_fixture("quoted_relation_dot", false);

    // Act
    assert_quoted_and_qualified_relations_are_distinct(&fixture);

    // Assert
    assert_eq!(
        fixture.rows("SELECT id FROM triage.dot"),
        vec![vec![cassie::types::Value::Int64(9)]]
    );
}

#[test]
fn should_distinguish_scoped_quoted_relation_from_schema_qualified_relation() {
    // Arrange
    let fixture = relation_fixture("quoted_relation_dot_scoped", true);

    // Act
    assert_quoted_and_qualified_relations_are_distinct(&fixture);

    // Assert
    assert_eq!(
        fixture.rows("SELECT id FROM triage.dot"),
        vec![vec![cassie::types::Value::Int64(9)]]
    );
}

#[test]
fn should_not_resolve_a_missing_quoted_dotted_relation_to_a_schema_qualified_relation() {
    // Arrange
    let fixture = relation_fixture("quoted_relation_no_fallback", false);
    fixture
        .execute("INSERT INTO triage.dot (id) VALUES (99)")
        .expect("insert schema-qualified relation");

    // Act
    let missing_quoted_relation = fixture.execute("SELECT id FROM \"triage.dot\"");

    // Assert
    assert!(
        missing_quoted_relation.is_err(),
        "missing quoted relation must not resolve to a schema-qualified relation"
    );
}

#[test]
fn should_preserve_quoted_relation_name_through_ddl_lifecycle() {
    // Arrange
    let fixture = relation_fixture("quoted_relation_rename_drop", false);
    fixture
        .execute("CREATE TABLE \"triage.rename\" (id BIGINT PRIMARY KEY)")
        .expect("create quoted relation");
    fixture
        .execute("INSERT INTO \"triage.rename\" (id) VALUES (1)")
        .expect("insert quoted relation");

    // Act
    let renamed = fixture.execute("ALTER TABLE \"triage.rename\" RENAME TO \"triage.renamed\"");
    assert!(renamed.is_ok(), "ALTER TABLE rename should succeed");
    let renamed_rows = fixture.rows("SELECT id FROM \"triage.renamed\"");
    let old_relation = fixture.execute("SELECT id FROM \"triage.rename\"");
    let dropped = fixture.execute("DROP TABLE \"triage.renamed\"");
    let renamed_metadata = fixture
        .cassie
        .midge
        .collection_metadata("postgres.public.\"triage.renamed\"")
        .expect("read renamed relation metadata");

    // Assert
    assert_eq!(renamed_rows, vec![vec![cassie::types::Value::Int64(1)]]);
    assert!(
        old_relation.is_err(),
        "old relation name should not resolve"
    );
    assert!(dropped.is_ok(), "DROP TABLE should succeed");
    assert!(
        renamed_metadata.is_none(),
        "renamed collection metadata should be removed"
    );
}

#[test]
fn should_preserve_quoted_view_name_through_ddl_lifecycle() {
    // Arrange
    let fixture = relation_fixture("quoted_view_component", false);
    fixture
        .execute("INSERT INTO triage.dot (id) VALUES (11)")
        .expect("insert source relation");

    // Act
    let created = fixture.execute("CREATE VIEW \"triage.view\" AS SELECT id FROM triage.dot");
    let view_rows = fixture.rows("SELECT id FROM \"triage.view\"");
    let dropped = fixture.execute("DROP VIEW \"triage.view\"");
    let after_drop = fixture.execute("SELECT id FROM \"triage.view\"");

    // Assert
    assert!(created.is_ok(), "CREATE VIEW should succeed");
    assert_eq!(view_rows, vec![vec![cassie::types::Value::Int64(11)]]);
    assert!(dropped.is_ok(), "DROP VIEW should succeed");
    assert!(after_drop.is_err(), "dropped view should not resolve");
}

fn relation_fixture(label: &str, scoped: bool) -> support_sql_fixture::SqlFixture {
    let fixture = sql_fixture(label, &[]);
    if scoped {
        fixture.cassie.catalog.register_database("postgres", None);
    }
    fixture
        .execute("CREATE SCHEMA triage")
        .expect("create triage schema");
    fixture
        .execute("CREATE TABLE triage.dot (id BIGINT PRIMARY KEY)")
        .expect("create schema-qualified table");
    fixture
}

fn assert_quoted_and_qualified_relations_are_distinct(fixture: &support_sql_fixture::SqlFixture) {
    // Act
    let created = fixture
        .execute("CREATE TABLE \"triage.dot\" (id BIGINT PRIMARY KEY, other_id BIGINT UNIQUE)");
    let created_index =
        fixture.execute("CREATE INDEX dotted_relation_other_id_idx ON \"triage.dot\" (other_id)");
    let created_foreign_key = fixture.execute(
        "CREATE TABLE dotted_relation_child (id BIGINT PRIMARY KEY, relation_id BIGINT REFERENCES \"triage.dot\"(other_id))",
    );
    let inserted_dotted =
        fixture.execute("INSERT INTO \"triage.dot\" (id, other_id) VALUES (7, 70)");
    let inserted_dotted_for_delete =
        fixture.execute("INSERT INTO \"triage.dot\" (id, other_id) VALUES (8, 80)");
    let inserted_qualified = fixture.execute("INSERT INTO triage.dot (id) VALUES (9)");
    let updated_dotted = fixture.execute("UPDATE \"triage.dot\" SET other_id = 71 WHERE id = 7");
    let deleted_dotted = fixture.execute("DELETE FROM \"triage.dot\" WHERE id = 8");
    let dotted_rows = fixture.rows("SELECT other_id FROM \"triage.dot\"");
    let qualified_rows = fixture.rows("SELECT id FROM triage.dot");

    // Assert
    assert!(created.is_ok(), "CREATE TABLE should succeed");
    assert!(created_index.is_ok(), "CREATE INDEX should succeed");
    assert!(
        created_foreign_key.is_ok(),
        "foreign key target should succeed"
    );
    assert!(inserted_dotted.is_ok(), "quoted INSERT should succeed");
    assert!(
        inserted_qualified.is_ok(),
        "qualified INSERT should succeed"
    );
    assert!(
        inserted_dotted_for_delete.is_ok(),
        "quoted INSERT for DELETE should succeed"
    );
    assert!(updated_dotted.is_ok(), "quoted UPDATE should succeed");
    assert!(deleted_dotted.is_ok(), "quoted DELETE should succeed");
    assert_eq!(dotted_rows, vec![vec![cassie::types::Value::Int64(71)]]);
    assert_eq!(qualified_rows, vec![vec![cassie::types::Value::Int64(9)]]);
    let dotted_relation = fixture
        .cassie
        .midge
        .collection_metadata("postgres.public.\"triage.dot\"")
        .expect("read dotted relation metadata")
        .expect("dotted relation metadata");
    let qualified_relation = fixture
        .cassie
        .midge
        .collection_metadata("postgres.triage.dot")
        .expect("read qualified relation metadata")
        .expect("qualified relation metadata");
    assert_ne!(dotted_relation.storage_id, qualified_relation.storage_id);
}

#[test]
fn should_preserve_qualified_dotted_field_values_outside_wire_profile() {
    // Arrange
    let fixture = sql_fixture("qualified_dotted_value_counterprobe", &[]);
    fixture.cassie.startup().expect("start Cassie");
    fixture
        .execute(
            "CREATE TABLE output_identifier_rows (gate BOOLEAN, \"Gate\" BOOLEAN, \"a.b\" INT)",
        )
        .expect("create stored-identifier source");
    fixture
        .execute(
            "INSERT INTO output_identifier_rows (gate, \"Gate\", \"a.b\") VALUES (TRUE, FALSE, 42)",
        )
        .expect("insert distinct stored values");

    // Act
    let bare = fixture
        .execute("SELECT \"a.b\" AS dotted FROM output_identifier_rows")
        .expect("execute bare dotted field outside wire profile");
    let qualified_gate = fixture
        .execute("SELECT output_identifier_rows.\"Gate\" AS flag FROM output_identifier_rows")
        .expect("execute ordinary qualified delimited field");
    let qualified_dotted = fixture
        .execute("SELECT output_identifier_rows.\"a.b\" AS dotted FROM output_identifier_rows")
        .expect("execute qualified dotted field outside wire profile");
    // Assert
    assert_eq!(bare.rows, vec![vec![cassie::types::Value::Int64(42)]]);
    assert_eq!(
        qualified_gate.rows,
        vec![vec![cassie::types::Value::Bool(false)]]
    );
    assert_eq!(
        qualified_dotted.rows,
        vec![vec![cassie::types::Value::Int64(42)]]
    );
}
