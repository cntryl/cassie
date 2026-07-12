use cassie::app::Cassie;
use cassie::types::Value;

#[path = "support/sql.rs"]
mod support;
use support::{data_dir, with_fallback};

#[test]
fn should_preserve_unknown_across_null_predicate_boolean_logic() {
    // Arrange
    with_fallback();
    let path = data_dir("null_semantics");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE null_semantics (row_key INT, score INT, title TEXT)",
            vec![],
        )
        .expect("create table");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO null_semantics (row_key, score, title) VALUES (1, NULL, NULL), (2, 0, 'alpha'), (3, 1, 'beta')",
            vec![],
        )
        .expect("seed rows");

    // Act
    let selected_ids = |predicate: &str| {
        cassie
            .execute_sql(
                &session,
                &format!("SELECT row_key FROM null_semantics WHERE {predicate} ORDER BY row_key"),
                vec![],
            )
            .expect("evaluate null predicate")
            .rows
            .into_iter()
            .map(|row| row[0].clone())
            .collect::<Vec<_>>()
    };

    // Assert
    assert_eq!(selected_ids("score = NULL"), Vec::<Value>::new());
    assert_eq!(selected_ids("(score + 1) = 1"), vec![Value::Int64(2)]);
    assert_eq!(
        selected_ids("score BETWEEN -1 AND 1"),
        vec![Value::Int64(2), Value::Int64(3)]
    );
    assert_eq!(selected_ids("score IN (1, NULL)"), vec![Value::Int64(3)]);
    assert_eq!(selected_ids("score NOT IN (1, NULL)"), Vec::<Value>::new());
    assert_eq!(selected_ids("NOT (score = 1)"), vec![Value::Int64(2)]);
    assert_eq!(
        selected_ids("NOT (score = 1 AND score <> 1)"),
        vec![Value::Int64(2), Value::Int64(3)]
    );
    assert_eq!(
        selected_ids("score = 1 OR score <> 1"),
        vec![Value::Int64(2), Value::Int64(3)]
    );
    assert_eq!(selected_ids("NOT (title LIKE 'a%')"), vec![Value::Int64(3)]);

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_incompatible_operands_with_division_by_zero() {
    // Arrange
    with_fallback();
    let path = data_dir("null_semantics_errors");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE null_semantics_errors (score INT, title TEXT)",
            vec![],
        )
        .expect("create table");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO null_semantics_errors (score, title) VALUES (4, 'four')",
            vec![],
        )
        .expect("seed row");

    // Act
    let incompatible = cassie.execute_sql(
        &session,
        "SELECT score FROM null_semantics_errors WHERE score = title",
        vec![],
    );
    let division = cassie.execute_sql(
        &session,
        "SELECT score FROM null_semantics_errors WHERE (score / 0) = 1",
        vec![],
    );

    // Assert
    assert!(incompatible
        .expect_err("incompatible comparison should fail")
        .to_string()
        .contains("incompatible"));
    assert_eq!(
        division
            .expect_err("division by zero should fail")
            .to_string(),
        "execution error: division by zero"
    );

    let _ = std::fs::remove_dir_all(path);
}
