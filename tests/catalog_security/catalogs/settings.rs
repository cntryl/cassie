//! PostgreSQL setting-option and configuration-flag helpers.

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_expand_options_given_null_and_populated_arrays_when_pg_options_to_table_is_called() {
    // Arrange
    let fixture = sql_fixture("issue-817-options-to-table", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let populated = fixture
        .execute(
            "SELECT option_name, option_value \
             FROM pg_catalog.pg_options_to_table( \
                 ARRAY['fillfactor=70', 'autovacuum_enabled=false']::TEXT[] \
             ) ORDER BY option_name",
        )
        .expect("expand a populated option array");
    let null_input = fixture
        .execute(
            "SELECT option_name, option_value \
             FROM pg_catalog.pg_options_to_table(NULL::TEXT[])",
        )
        .expect("expand a NULL option array as an empty set");

    // Assert
    let expected_types = [25, 25];
    assert_eq!(
        populated
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        expected_types
    );
    assert_eq!(
        populated.rows(),
        &[
            vec![
                Value::String("autovacuum_enabled".to_owned()),
                Value::String("false".to_owned()),
            ],
            vec![
                Value::String("fillfactor".to_owned()),
                Value::String("70".to_owned()),
            ],
        ]
    );
    assert_eq!(
        null_input
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        expected_types
    );
    assert_eq!(null_input.rows(), &[] as &[Vec<Value>]);
}

#[test]
fn should_report_setting_flags_given_known_and_unknown_settings_when_pg_settings_get_flags_is_called(
) {
    // Arrange
    let fixture = sql_fixture("issue-817-setting-flags", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let result = fixture
        .execute(
            "SELECT pg_catalog.pg_settings_get_flags('work_mem')::text, \
                    pg_catalog.pg_settings_get_flags('issue_817_unknown_setting') IS NULL, \
                    pg_catalog.pg_settings_get_flags('work_mem'), \
                    pg_catalog.pg_settings_get_flags(NULL) IS NULL",
        )
        .expect("read flags for one known and one missing configuration setting");

    // Assert
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        [25, 16, 1_009, 16]
    );
    assert_eq!(
        result.rows(),
        &[vec![
            Value::String("{EXPLAIN}".to_owned()),
            Value::Bool(true),
            Value::Json(serde_json::json!(["EXPLAIN"])),
            Value::Bool(true),
        ]]
    );
}
