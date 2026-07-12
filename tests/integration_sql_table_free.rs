use cassie::app::Cassie;
use cassie::types::Value;

#[path = "support/sql.rs"]
mod support;
use support::{data_dir, with_fallback};

#[test]
fn should_execute_table_free_literal_alias_cast_projection() {
    // Arrange
    with_fallback();
    let path = data_dir("table_free_literals");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
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
            .columns
            .iter()
            .map(|column| (column.name.as_str(), column.type_oid))
            .collect::<Vec<_>>(),
        vec![
            ("one", 701),
            ("label", 25),
            ("enabled", 16),
            ("missing", 705),
            ("two", 23),
            ("total", 701),
        ]
    );
    assert_eq!(
        result.rows,
        vec![vec![
            Value::Float64(1.0),
            Value::String("alpha".to_string()),
            Value::Bool(true),
            Value::Null,
            Value::Int64(2),
            Value::Float64(3.0),
        ]]
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_execute_table_free_parameter_through_union_all() {
    // Arrange
    with_fallback();
    let path = data_dir("table_free_parameters");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
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
    assert_eq!(result.columns[0].type_oid, 23);
    assert_eq!(
        result.rows,
        vec![vec![Value::Float64(2.0)], vec![Value::Int64(7)]]
    );

    let _ = std::fs::remove_dir_all(path);
}
