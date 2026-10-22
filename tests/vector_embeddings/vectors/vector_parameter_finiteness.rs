use cassie::types::{Value, Vector};

#[test]
fn should_reject_non_finite_vector_parameter_components_without_writing() {
    // Arrange
    let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE vector_param_json (label TEXT, doc JSON)",
            Vec::new(),
        )
        .expect("create table");
    let insert = "INSERT INTO vector_param_json (label, doc) VALUES ($1, $2)";

    // Act
    let infinite = cassie.execute_sql(
        &session,
        insert,
        vec![
            Value::String("inf".to_string()),
            Value::Vector(Vector::new(vec![1.0, f32::INFINITY, 3.0])),
        ],
    );
    let nan = cassie.execute_sql(
        &session,
        insert,
        vec![
            Value::String("nan".to_string()),
            Value::Vector(Vector::new(vec![f32::NAN])),
        ],
    );
    cassie
        .execute_sql(
            &session,
            "INSERT INTO vector_param_json (label, doc) VALUES ('kept', '[1]')",
            Vec::new(),
        )
        .expect("insert finite row");
    let updated = cassie.execute_sql(
        &session,
        "UPDATE vector_param_json SET doc = $1",
        vec![Value::Vector(Vector::new(vec![2.0, f32::NEG_INFINITY]))],
    );
    let result = cassie
        .execute_sql(
            &session,
            "SELECT label, doc FROM vector_param_json",
            Vec::new(),
        )
        .expect("select rows");
    let rows = result.rows();

    // Assert
    assert!(infinite.is_err(), "infinite component must be rejected");
    assert!(nan.is_err(), "NaN component must be rejected");
    assert!(
        updated.is_err(),
        "non-finite UPDATE component must be rejected"
    );
    assert_eq!(
        rows,
        vec![vec![
            Value::String("kept".to_string()),
            Value::Json(serde_json::json!([1])),
        ]]
    );
    drop(cassie);
}
