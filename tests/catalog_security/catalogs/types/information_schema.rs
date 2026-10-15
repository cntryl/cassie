#[test]
fn should_report_array_element_metadata_in_information_schema() {
    // Arrange
    let fixture = sql_fixture(
        "issue-817-information-schema-element-types",
        &["CREATE TABLE issue_817_element_types (id INTEGER, labels INTEGER[])"],
    );
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT object_catalog, object_schema, object_name, object_type, \
                    collection_type_identifier, data_type, udt_catalog, udt_schema, \
                    udt_name, dtd_identifier \
             FROM information_schema.element_types \
             WHERE object_catalog = current_database() \
               AND object_schema = 'public' \
               AND object_name = 'issue_817_element_types'",
        )
        .expect("read the declared array element metadata");

    // Assert
    assert_descriptor(
        &selected,
        &[
            ("object_catalog", PG18_SQL_IDENTIFIER_OID, 64, -1, true),
            ("object_schema", PG18_SQL_IDENTIFIER_OID, 64, -1, true),
            ("object_name", PG18_SQL_IDENTIFIER_OID, 64, -1, true),
            ("object_type", PG18_CHARACTER_DATA_OID, -1, -1, true),
            (
                "collection_type_identifier",
                PG18_SQL_IDENTIFIER_OID,
                64,
                -1,
                true,
            ),
            ("data_type", PG18_CHARACTER_DATA_OID, -1, -1, true),
            ("udt_catalog", PG18_SQL_IDENTIFIER_OID, 64, -1, true),
            ("udt_schema", PG18_SQL_IDENTIFIER_OID, 64, -1, true),
            ("udt_name", PG18_SQL_IDENTIFIER_OID, 64, -1, true),
            ("dtd_identifier", PG18_SQL_IDENTIFIER_OID, 64, -1, true),
        ],
    );
    assert_eq!(
        selected.rows(),
        &[vec![
            Value::String("postgres".to_owned()),
            Value::String("public".to_owned()),
            Value::String("issue_817_element_types".to_owned()),
            Value::String("TABLE".to_owned()),
            Value::String("2".to_owned()),
            Value::String("integer".to_owned()),
            Value::String("postgres".to_owned()),
            Value::String("pg_catalog".to_owned()),
            Value::String("int4".to_owned()),
            Value::String("a2".to_owned()),
        ]]
    );
}
