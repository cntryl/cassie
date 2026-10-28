use cassie::types::Value;

use super::support_sql as support;
use support::data_dir;

#[test]
fn should_execute_table_free_literal_alias_cast_projection() {
    // Arrange
    let path = data_dir("table_free_literals");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);

    // Act
    let result = cassie
        .execute_sql(
            &session,
            "SELECT 1 AS one, 'alpha' AS label, true AS enabled, NULL AS missing, CAST(2 AS INT) AS two, 1 + 2 AS total",
            vec![],
        )
        .expect("execute table-free projection");

    // Assert
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| (column.name.as_str(), column.type_oid))
            .collect::<Vec<_>>(),
        vec![
            ("one", 23),
            ("label", 25),
            ("enabled", 16),
            ("missing", 705),
            ("two", 23),
            // Integer arithmetic preserves its resolved native width.
            ("total", 23),
        ]
    );
    assert_eq!(
        result.rows(),
        vec![vec![
            Value::Int64(1),
            Value::String("alpha".to_string()),
            Value::Bool(true),
            Value::Null,
            Value::Int64(2),
            Value::Int64(3),
        ]]
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_execute_table_free_parameter_through_union_all() {
    // Arrange
    let path = data_dir("table_free_parameters");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);

    // Act
    let result = cassie
        .execute_sql(
            &session,
            "SELECT $1::INT AS value UNION ALL SELECT 2 AS value",
            vec![Value::String("7".to_string())],
        )
        .expect("execute table-free set operation");

    // Assert
    assert_eq!(result.columns()[0].type_oid, 23);
    assert_eq!(
        result.rows(),
        vec![vec![Value::Int64(2)], vec![Value::Int64(7)]]
    );

    let _ = std::fs::remove_dir_all(path);
}
