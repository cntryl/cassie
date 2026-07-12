use std::path::PathBuf;

use cassie::app::Cassie;
use cassie::config::CassieRuntimeConfig;
use cassie::sql::ast::{CteQuery, QueryStatement, SetOperator};
use cassie::sql::parser::parse_statement;
use cassie::types::{DataType, FieldSchema, Schema, Value};

#[path = "support/sql.rs"]
mod support;
use support::{data_dir, with_fallback};

fn seeded_integer_collection(name: &str, values: &[i64]) -> (Cassie, PathBuf) {
    with_fallback();
    let path = data_dir(name);
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    let collection = format!("{name}_seed");
    let schema = Schema {
        fields: vec![FieldSchema {
            name: "n".to_string(),
            data_type: DataType::Int,
            nullable: true,
        }],
    };
    cassie
        .midge
        .create_collection(&collection, schema.clone())
        .expect("create seed collection");
    cassie.register_collection(
        &collection,
        schema
            .fields
            .iter()
            .map(|field| (field.name.clone(), field.data_type.clone()))
            .collect(),
    );
    for (index, value) in values.iter().enumerate() {
        cassie
            .midge
            .put_document(
                &collection,
                Some(format!("d{index}")),
                serde_json::json!({"n": value}),
            )
            .expect("insert seed row");
    }
    (cassie, path.into())
}

fn integer_values(result: cassie::executor::QueryResult) -> Vec<i64> {
    result
        .rows
        .into_iter()
        .map(|row| match row.first() {
            Some(Value::Int64(value)) => *value,
            Some(Value::Float64(value)) => value
                .to_string()
                .parse::<i64>()
                .expect("expected an integral float"),
            other => panic!("expected integer value, got {other:?}"),
        })
        .collect()
}

#[test]
fn should_preserve_recursive_union_operator_in_ast() {
    // Arrange
    let sql = "WITH RECURSIVE seq(n) AS (SELECT 1 UNION SELECT n + 1 FROM seq WHERE n < 2) SELECT n FROM seq";

    // Act
    let parsed = parse_statement(sql).expect("parse recursive union");

    // Assert
    let QueryStatement::Select(select) = parsed.statement else {
        panic!("expected SELECT");
    };
    let CteQuery::Recursive { operator, .. } = &select.ctes[0].query else {
        panic!("expected recursive CTE");
    };
    assert_eq!(*operator, SetOperator::Union);
}

#[test]
fn should_preserve_recursive_union_all_duplicates() {
    // Arrange
    let (cassie, path) = seeded_integer_collection("recursive_union_all", &[1, 1]);
    let session = cassie.create_session("tester", None);

    // Act
    let result = cassie
        .execute_sql(
            &session,
            "WITH RECURSIVE seq(n) AS (SELECT n FROM recursive_union_all_seed UNION ALL SELECT n + 1 AS n FROM seq WHERE n < 2) SELECT n FROM seq ORDER BY n",
            vec![],
        )
        .expect("execute recursive UNION ALL");

    // Assert
    assert_eq!(integer_values(result), vec![1, 1, 2, 2]);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_deduplicate_recursive_union_rows() {
    // Arrange
    let (cassie, path) = seeded_integer_collection("recursive_union", &[1, 1]);
    let session = cassie.create_session("tester", None);

    // Act
    let result = cassie
        .execute_sql(
            &session,
            "WITH RECURSIVE seq(n) AS (SELECT n FROM recursive_union_seed UNION SELECT n + 1 AS n FROM seq WHERE n < 2) SELECT n FROM seq ORDER BY n",
            vec![],
        )
        .expect("execute recursive UNION");

    // Assert
    assert_eq!(integer_values(result), vec![1, 2]);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_apply_recursive_cte_column_aliases() {
    // Arrange
    let (cassie, path) = seeded_integer_collection("recursive_alias", &[1]);
    let session = cassie.create_session("tester", None);

    // Act
    let result = cassie
        .execute_sql(
            &session,
            "WITH RECURSIVE seq(value) AS (SELECT n FROM recursive_alias_seed UNION ALL SELECT value + 1 FROM seq WHERE value < 2) SELECT value FROM seq ORDER BY value",
            vec![],
        )
        .expect("execute recursive CTE with aliases");

    // Assert
    assert_eq!(result.columns[0].name, "value");
    assert_eq!(integer_values(result), vec![1, 2]);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_pass_parameters_through_recursive_terms() {
    // Arrange
    with_fallback();
    let path = data_dir("recursive_parameters");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    let session = cassie.create_session("tester", None);

    // Act
    let result = cassie
        .execute_sql(
            &session,
            "WITH RECURSIVE seq(n) AS (SELECT $1::INT UNION ALL SELECT n + 1 FROM seq WHERE n < $2::INT) SELECT n FROM seq ORDER BY n",
            vec![Value::String("1".to_string()), Value::String("3".to_string())],
        )
        .expect("execute parameterized recursive CTE");

    // Assert
    assert_eq!(integer_values(result), vec![1, 2, 3]);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_recursive_anchor_self_reference() {
    // Arrange
    let (cassie, path) = seeded_integer_collection("recursive_anchor_self", &[1]);
    let session = cassie.create_session("tester", None);

    // Act
    let result = cassie.execute_sql(
        &session,
        "WITH RECURSIVE seq(n) AS (SELECT n FROM seq UNION ALL SELECT n + 1 FROM seq WHERE n < 2) SELECT n FROM seq",
        vec![],
    );

    // Assert
    let error = result.expect_err("anchor self-reference must be rejected");
    assert!(error.to_string().contains("anchor"), "error={error}");
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_recursive_projection_shape_mismatch() {
    // Arrange
    let (cassie, path) = seeded_integer_collection("recursive_shape", &[1]);
    let session = cassie.create_session("tester", None);

    // Act
    let result = cassie.execute_sql(
        &session,
        "WITH RECURSIVE seq(n) AS (SELECT n FROM recursive_shape_seed UNION ALL SELECT n, n FROM seq WHERE n < 2) SELECT n FROM seq",
        vec![],
    );

    // Assert
    let error = result.expect_err("recursive arity mismatch must be rejected");
    assert!(error.to_string().contains("column count"), "error={error}");
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_recursive_projection_type_mismatch() {
    // Arrange
    let (cassie, path) = seeded_integer_collection("recursive_type", &[1]);
    let session = cassie.create_session("tester", None);

    // Act
    let result = cassie.execute_sql(
        &session,
        "WITH RECURSIVE seq(n) AS (SELECT n FROM recursive_type_seed UNION ALL SELECT 'wrong' FROM seq WHERE n < 2) SELECT n FROM seq",
        vec![],
    );

    // Assert
    let error = result.expect_err("recursive type mismatch must be rejected");
    assert!(error.to_string().contains("type"), "error={error}");
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_multiple_recursive_references() {
    // Arrange
    let (cassie, path) = seeded_integer_collection("recursive_multiple", &[1]);
    let session = cassie.create_session("tester", None);

    // Act
    let result = cassie.execute_sql(
        &session,
        "WITH RECURSIVE seq(n) AS (SELECT n FROM recursive_multiple_seed UNION ALL SELECT seq.n FROM seq JOIN seq ON seq.n = seq.n WHERE seq.n < 2) SELECT n FROM seq",
        vec![],
    );

    // Assert
    let error = result.expect_err("multiple recursive references must be rejected");
    assert!(error.to_string().contains("multiple"), "error={error}");
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_recursive_working_table_over_temp_budget() {
    // Arrange
    with_fallback();
    let path = data_dir("recursive_temp_budget");
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.temp_spill_budget_bytes = 1;
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).expect("create Cassie");
    let session = cassie.create_session("tester", None);

    // Act
    let result = cassie.execute_sql(
        &session,
        "WITH RECURSIVE seq(n) AS (SELECT 1 UNION ALL SELECT CAST(n + 1 AS INT) FROM seq WHERE n < 3) SELECT n FROM seq",
        vec![],
    );

    // Assert
    let error = result.expect_err("recursive working table should honor temp budget");
    assert!(
        error
            .to_string()
            .contains("temporary storage budget exceeded"),
        "error={error}"
    );
    let _ = std::fs::remove_dir_all(path);
}
