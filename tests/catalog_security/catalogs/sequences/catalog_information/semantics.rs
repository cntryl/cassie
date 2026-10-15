#[test]
fn should_report_serial_sequence_dependency_on_created_table() {
    // Arrange
    let fixture = created_object_catalog_fixture("issue-817-created-sequence-dependency");

    // Act
    let dependency = fixture
        .execute(
            "SELECT d.deptype::text, \
                    d.refobjid = 'issue_817_catalog_probe.items'::regclass, \
                    d.refobjsubid \
             FROM pg_catalog.pg_depend AS d \
             WHERE d.classid = 'pg_class'::regclass \
               AND d.refclassid = 'pg_class'::regclass \
               AND d.objid = pg_catalog.pg_get_serial_sequence( \
                   'issue_817_catalog_probe.items', 'id')::regclass",
        )
        .expect("read serial sequence dependency");

    // Assert
    assert_eq!(
        dependency.rows(),
        vec![vec![
            Value::String("a".to_owned()),
            Value::Bool(true),
            Value::Int64(1),
        ]]
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
