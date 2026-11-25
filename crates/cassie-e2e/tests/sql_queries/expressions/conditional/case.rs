use super::support_sql::data_dir;
use cassie::types::Value;

#[test]
fn should_evaluate_nested_case_with_nulls() {
    // Arrange
    let path = data_dir("case_nested_null");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);

    // Act
    let result = cassie.execute_sql(&session,
            "SELECT CASE NULL WHEN NULL THEN 'wrong' ELSE CASE WHEN NULL THEN 'wrong' WHEN true THEN 'chosen' END END AS chosen, CASE 2 WHEN 1 THEN 'wrong' WHEN 2 THEN 'two' END AS two, CASE WHEN false THEN 1 END AS missing, CASE WHEN true THEN 2 ELSE 0 END + 1 AS arithmetic, CASE WHEN true THEN 1 ELSE 1 / 0 END AS lazy",
            vec![]).expect("CASE query");

    // Assert
    assert_eq!(
        result.rows(),
        vec![vec![
            Value::String("chosen".into()),
            Value::String("two".into()),
            Value::Null,
            Value::Int64(3),
            Value::Int64(1)
        ]]
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_execute_case_in_relational_contexts() {
    // Arrange
    let path = data_dir("case_relational");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE case_relational (score INT, label TEXT)",
            vec![],
        )
        .unwrap();
    cassie
        .execute_sql(
            &session,
            "INSERT INTO case_relational (score, label) VALUES (1, 'one')",
            vec![],
        )
        .unwrap();
    cassie
        .execute_sql(
            &session,
            "INSERT INTO case_relational (score, label) VALUES (2, 'two')",
            vec![],
        )
        .unwrap();

    // Act
    let rows = cassie.execute_sql(&session,
            "SELECT CASE score WHEN $1 THEN label ELSE 'other' END AS bucket FROM case_relational WHERE CASE WHEN score > 0 THEN true ELSE false END = true ORDER BY CASE WHEN score = 2 THEN 0 ELSE 1 END",
            vec![Value::Int64(2)]).expect("CASE query");
    let aggregate = cassie
        .execute_sql(
            &session,
            "SELECT SUM(CASE WHEN score > 1 THEN score ELSE 0 END) AS total FROM case_relational",
            vec![],
        )
        .expect("CASE aggregate");

    // Assert
    assert_eq!(
        rows.rows(),
        vec![
            vec![Value::String("two".into())],
            vec![Value::String("other".into())]
        ]
    );
    assert_eq!(aggregate.rows(), vec![vec![Value::Int64(2)]]);
    assert_eq!(aggregate.columns()[0].type_oid, 20);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_invalid_case_forms() {
    // Arrange
    let path = data_dir("case_errors");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);

    // Act
    let missing_then = cassie.execute_sql(&session, "SELECT CASE WHEN true 1 END", vec![]);
    let incompatible = cassie.execute_sql(
        &session,
        "SELECT CASE WHEN true THEN 1 ELSE 'text' END",
        vec![],
    );
    let non_boolean = cassie.execute_sql(&session, "SELECT CASE WHEN 1 THEN 'x' END", vec![]);
    let incompatible_temporal = cassie.execute_sql(
            &session,
            "SELECT CASE WHEN true THEN CAST('2024-01-01' AS DATE) ELSE CAST('2024-01-01T00:00:00Z' AS TIMESTAMP) END",
            vec![],
        );

    // Assert
    assert!(missing_then.is_err());
    assert!(incompatible
        .unwrap_err()
        .to_string()
        .contains("CASE result"));
    assert!(non_boolean
        .unwrap_err()
        .to_string()
        .contains("argument of CASE/WHEN must be type boolean, not type integer"));
    assert!(incompatible_temporal
        .unwrap_err()
        .to_string()
        .contains("CASE result"));
    let _ = std::fs::remove_dir_all(path);
}
