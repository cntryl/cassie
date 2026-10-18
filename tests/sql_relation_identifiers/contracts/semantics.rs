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
    assert_eq!(bare.rows(), vec![vec![cassie::types::Value::Int64(42)]]);
    assert_eq!(
        qualified_gate.rows(),
        vec![vec![cassie::types::Value::Bool(false)]]
    );
    assert_eq!(
        qualified_dotted.rows(),
        vec![vec![cassie::types::Value::Int64(42)]]
    );
}
