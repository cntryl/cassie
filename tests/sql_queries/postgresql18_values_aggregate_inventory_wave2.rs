//! PostgreSQL 18 query contracts that were missing from the focused owner suites.

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_apply_interval_range_offsets_to_timestamp_window_frames() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-timestamp-range-window-interval",
        &["CREATE TABLE issue_806_timestamp_window (instant TIMESTAMP, id INT, amount INT)"],
    );
    fixture
        .execute("INSERT INTO issue_806_timestamp_window VALUES ('2024-03-09 11:30:00', 1, 7), ('2024-03-09 12:00:00', 2, 10), ('2024-03-10 12:00:00', 3, 20), ('2024-03-11 12:00:00', 4, 30)")
        .unwrap_or_else(|error| panic!("timestamp table fixture insert failed: {error:?}"));

    // Act
    let result = fixture.execute(
        "SELECT id, FIRST_VALUE(id) OVER (ORDER BY instant RANGE BETWEEN INTERVAL '1 day' PRECEDING AND CURRENT ROW) FROM issue_806_timestamp_window ORDER BY instant",
    );

    // Assert
    let result = result.expect("timestamp RANGE frames accept interval offsets");
    assert_eq!(
        result.rows(),
        &[
            vec![Value::Int64(1), Value::Int64(1)],
            vec![Value::Int64(2), Value::Int64(1)],
            vec![Value::Int64(3), Value::Int64(2)],
            vec![Value::Int64(4), Value::Int64(3)],
        ]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        [23, 23]
    );
}

#[test]
fn should_zip_multiple_unnest_arrays_with_ordinality_null_padding() {
    // Arrange
    let fixture = sql_fixture("issue-808-multiple-unnest-ordinality-padding", &[]);

    // Act
    let result = fixture.execute(
        "SELECT expanded.number, expanded.label, expanded.ordinality FROM unnest('{10,20}'::INT[], '{a,b,c}'::TEXT[]) WITH ORDINALITY AS expanded(number, label, ordinality) ORDER BY expanded.ordinality",
    );

    // Assert
    let result = result.expect("multi-array UNNEST zips inputs and pads shorter arrays with NULL");
    assert_eq!(
        result.rows(),
        &[
            vec![
                Value::Int64(10),
                Value::String("a".to_owned()),
                Value::Int64(1),
            ],
            vec![
                Value::Int64(20),
                Value::String("b".to_owned()),
                Value::Int64(2),
            ],
            vec![Value::Null, Value::String("c".to_owned()), Value::Int64(3),],
        ]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        [23, 25, 20]
    );
}

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
