//! Selected array table-source behavior owned by #808.

use crate::support_sql_fixture::sql_fixture;
use cassie::app::CassieError;
use cassie::types::Value;

#[test]
fn should_evaluate_array_subquery_arguments_at_their_source_invocation() {
    // Arrange
    let fixture = sql_fixture("issue-808-unnest-subquery-arguments", &[]);

    // Act
    let independent = fixture.execute("SELECT n FROM unnest(ARRAY(SELECT 7)) s(n)");
    let lateral = fixture.execute(
        "SELECT v.k,n FROM (VALUES(1),(2)) v(k) CROSS JOIN LATERAL unnest(ARRAY(SELECT v.k)) s(n)",
    );

    // Assert
    assert_eq!(
        independent.expect("ARRAY subquery source argument").rows(),
        &[vec![Value::Int64(7)]]
    );
    assert_eq!(
        lateral.expect("correlated ARRAY source argument").rows(),
        &[
            vec![Value::Int64(1), Value::Int64(1)],
            vec![Value::Int64(2), Value::Int64(2)]
        ]
    );
}

#[test]
fn should_unnest_multidimensional_arrays_in_row_major_order_with_ordinality() {
    // Arrange
    let fixture = sql_fixture("issue-808-unnest-rectangular", &[]);

    // Act
    let result = fixture.execute(
        "SELECT item.value, item.position FROM unnest(ARRAY[[1,NULL],[3,4]]) WITH ORDINALITY AS item(value,position)",
    );

    // Assert
    assert_eq!(
        result.expect("row-major UNNEST").rows(),
        &[
            vec![Value::Int64(1), Value::Int64(1)],
            vec![Value::Null, Value::Int64(2)],
            vec![Value::Int64(3), Value::Int64(3)],
            vec![Value::Int64(4), Value::Int64(4)],
        ]
    );
}

#[test]
fn should_pad_shorter_arrays_with_nulls_when_unnesting_multiple_arrays() {
    // Arrange
    let fixture = sql_fixture("issue-808-unnest-zip", &[]);

    // Act
    let result = fixture.execute(
        "SELECT item.n, item.label, item.position FROM unnest(ARRAY[1,2], ARRAY['a']) WITH ORDINALITY AS item(n,label,position)",
    );

    // Assert
    assert_eq!(
        result.expect("unequal-length UNNEST").rows(),
        &[
            vec![
                Value::Int64(1),
                Value::String("a".to_owned()),
                Value::Int64(1)
            ],
            vec![Value::Int64(2), Value::Null, Value::Int64(2)],
        ]
    );
}

#[test]
fn should_preserve_outer_rows_when_lateral_unnest_has_empty_or_null_arrays() {
    // Arrange
    let fixture = sql_fixture(
        "issue-808-unnest-left-lateral",
        &[
            "CREATE TABLE unnest_inputs (k INT, items INT[])",
            "INSERT INTO unnest_inputs VALUES (1, ARRAY[10,20]), (2, ARRAY[]::INT[]), (3, NULL)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT unnest_inputs.k, item.value, item.position FROM unnest_inputs LEFT JOIN LATERAL unnest(unnest_inputs.items) WITH ORDINALITY AS item(value,position) ON TRUE ORDER BY unnest_inputs.k, item.position",
    );

    // Assert
    assert_eq!(
        result.expect("LEFT LATERAL UNNEST").rows(),
        &[
            vec![Value::Int64(1), Value::Int64(10), Value::Int64(1)],
            vec![Value::Int64(1), Value::Int64(20), Value::Int64(2)],
            vec![Value::Int64(2), Value::Null, Value::Null],
            vec![Value::Int64(3), Value::Null, Value::Null],
        ]
    );
}

#[test]
fn should_distinguish_null_filled_arrays_from_null_array_arguments() {
    // Arrange
    let fixture = sql_fixture("issue-807-null-array-fill", &[]);

    // Act
    let result = fixture.execute("SELECT array_fill(NULL::INT, ARRAY[2])::TEXT, cardinality(array_fill(NULL::INT, ARRAY[2])), cardinality(NULL::INT[])");

    // Assert
    assert_eq!(
        result.expect("NULL filled native array").rows(),
        &[vec![
            Value::String("{NULL,NULL}".to_owned()),
            Value::Int64(2),
            Value::Null
        ]]
    );
}

#[test]
fn should_describe_unnest_outputs_using_the_resolved_source_shape() {
    // Arrange
    let fixture = sql_fixture("issue-808-unnest-default-names", &[]);
    let cases = [
        ("SELECT * FROM unnest(ARRAY[1])", vec!["unnest"], vec![23]),
        (
            "SELECT unnest FROM unnest(ARRAY[1])",
            vec!["unnest"],
            vec![23],
        ),
        ("SELECT x FROM unnest(ARRAY[1]) AS x", vec!["x"], vec![23]),
        (
            "SELECT * FROM unnest(ARRAY[1], ARRAY['a'])",
            vec!["unnest", "unnest"],
            vec![23, 25],
        ),
        (
            "SELECT * FROM unnest(ARRAY[1]) WITH ORDINALITY AS x",
            vec!["x", "ordinality"],
            vec![23, 20],
        ),
    ];

    for (sql, labels, oids) in cases {
        // Act
        let result = fixture.execute(sql).expect("selected source labels");
        // Assert
        assert_eq!(
            result
                .columns()
                .iter()
                .map(|column| column.name.as_str())
                .collect::<Vec<_>>(),
            labels,
            "{sql}"
        );
        // Assert
        assert_eq!(
            result
                .columns()
                .iter()
                .map(|column| column.type_oid)
                .collect::<Vec<_>>(),
            oids,
            "{sql}"
        );
    }
}

#[test]
fn should_reject_invalid_unnest_bindings_with_native_diagnostics() {
    // Arrange
    let fixture = sql_fixture("issue-808-unnest-errors", &[]);
    let cases = [
        ("SELECT * FROM unnest(NULL)", "42725"),
        ("SELECT * FROM unnest('{}')", "42725"),
        ("SELECT unnest FROM unnest(ARRAY[1],ARRAY[2])", "42702"),
        ("SELECT * FROM unnest(ARRAY[1]) s(a,b)", "42P10"),
    ];

    for (sql, state) in cases {
        // Act
        let error = fixture.execute(sql).expect_err("selected source error");
        // Assert
        let matches = match state {
            "42725" => matches!(error, CassieError::AmbiguousFunction(_)),
            "42702" => matches!(error, CassieError::AmbiguousColumn(_)),
            "42P10" => {
                matches!(&error, CassieError::Planner(message) if message == "relation alias list has more names than source columns")
            }
            _ => unreachable!("fixed diagnostic cases"),
        };
        assert!(matches, "{sql}: expected {state}, received {error}");
    }
}
