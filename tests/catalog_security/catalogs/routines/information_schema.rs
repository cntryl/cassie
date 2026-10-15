#[test]
fn should_report_builtin_parameter_metadata_in_information_schema() {
    // Arrange
    let fixture = sql_fixture("issue-817-information-schema-parameters", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT specific_catalog, specific_schema, specific_name, ordinal_position, \
                    parameter_mode, is_result, parameter_name, data_type, udt_name, \
                    parameter_default \
             FROM information_schema.parameters \
             WHERE specific_schema = 'pg_catalog' \
               AND specific_name LIKE 'pg_sleep_for%' \
             ORDER BY ordinal_position",
        )
        .expect("read the built-in pg_sleep_for parameter metadata");

    // Assert
    assert_descriptor(
        &selected,
        &[
            ("specific_catalog", PG18_SQL_IDENTIFIER_OID, 64, -1, true),
            ("specific_schema", PG18_SQL_IDENTIFIER_OID, 64, -1, true),
            ("specific_name", PG18_SQL_IDENTIFIER_OID, 64, -1, true),
            ("ordinal_position", PG18_CARDINAL_NUMBER_OID, 4, -1, true),
            ("parameter_mode", PG18_CHARACTER_DATA_OID, -1, -1, true),
            ("is_result", PG18_YES_OR_NO_OID, -1, -1, true),
            ("parameter_name", PG18_SQL_IDENTIFIER_OID, 64, -1, true),
            ("data_type", PG18_CHARACTER_DATA_OID, -1, -1, true),
            ("udt_name", PG18_SQL_IDENTIFIER_OID, 64, -1, true),
            ("parameter_default", PG18_CHARACTER_DATA_OID, -1, -1, true),
        ],
    );
    assert_eq!(
        selected.rows(),
        &[vec![
            Value::String("postgres".to_owned()),
            Value::String("pg_catalog".to_owned()),
            Value::String("pg_sleep_for_3935".to_owned()),
            Value::Int64(1),
            Value::String("IN".to_owned()),
            Value::String("NO".to_owned()),
            Value::String(String::new()),
            Value::String("interval".to_owned()),
            Value::String("interval".to_owned()),
            Value::Null,
        ]]
    );
}
