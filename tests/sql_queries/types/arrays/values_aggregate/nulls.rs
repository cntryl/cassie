#[test]
fn should_preserve_any_all_truth_tables_for_null_empty_table_arrays() {
    // Arrange
    let fixture = sql_fixture(
        "issue-807-array-quantifier-null-bounds",
        &["CREATE TABLE issue_807_array_quantifier_values (id INT PRIMARY KEY, items INT[])"],
    );
    fixture
        .cassie
        .execute_sql(
            &fixture.session,
            "INSERT INTO issue_807_array_quantifier_values VALUES (1, $1), (2, $2), (3, $3), (4, NULL)",
            vec![
                Value::Json(serde_json::json!([1, null])),
                Value::Json(serde_json::json!([2, null])),
                Value::Json(serde_json::json!([])),
            ],
        )
        .unwrap_or_else(|error| panic!("array table fixture insert failed: {error:?}"));

    // Act
    let truth_table = fixture.execute(
        "SELECT id, 2 = ANY(items), 2 = ALL(items), array_lower(items, 1), array_upper(items, 1), cardinality(items) FROM issue_807_array_quantifier_values ORDER BY id",
    );

    // Assert
    let truth_table = truth_table.expect("array quantifiers retain SQL NULL and empty-set rules");
    assert_eq!(
        truth_table.rows(),
        &[
            vec![
                Value::Int64(1),
                Value::Null,
                Value::Bool(false),
                Value::Int64(1),
                Value::Int64(2),
                Value::Int64(2),
            ],
            vec![
                Value::Int64(2),
                Value::Bool(true),
                Value::Null,
                Value::Int64(1),
                Value::Int64(2),
                Value::Int64(2),
            ],
            vec![
                Value::Int64(3),
                Value::Bool(false),
                Value::Bool(true),
                Value::Null,
                Value::Null,
                Value::Int64(0),
            ],
            vec![
                Value::Int64(4),
                Value::Null,
                Value::Null,
                Value::Null,
                Value::Null,
                Value::Null,
            ],
        ]
    );
    assert_eq!(
        truth_table
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        [23, 16, 16, 23, 23, 23]
    );
}
