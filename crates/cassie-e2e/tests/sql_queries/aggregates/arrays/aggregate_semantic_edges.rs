#[test]
fn should_filter_distinct_array_aggregation_by_ordered_inputs() {
    // Arrange
    let fixture = sql_fixture(
        "issue-811-array-aggregate-distinct-order-filter",
        &[
            "CREATE TABLE issue_811_ordered_array_values (position INT, selected BOOLEAN, value INT)",
            "INSERT INTO issue_811_ordered_array_values VALUES (1,TRUE,2),(2,FALSE,99),(3,TRUE,NULL),(4,TRUE,1),(5,TRUE,2),(6,TRUE,NULL)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT ARRAY_AGG(DISTINCT value ORDER BY value DESC) FILTER (WHERE selected)::TEXT FROM issue_811_ordered_array_values",
    );

    // Assert
    let result = result.expect("array_agg filters before DISTINCT and aggregate-local ordering");
    assert_eq!(
        result.rows(),
        &[vec![Value::String("{NULL,2,1}".to_owned())]]
    );
    assert_eq!(result.columns()[0].type_oid, 25);
}

#[test]
fn should_reject_invalid_array_aggregate_input_shapes() {
    // Arrange
    let fixture = sql_fixture("issue-811-array-aggregate-input-shapes", &[]);
    let cases = [
        (
            "SELECT array_agg(value) FROM (VALUES (NULL::INT[])) AS inputs(value)",
            "22004",
            "cannot accumulate null arrays",
        ),
        (
            "SELECT array_agg(value) FROM (VALUES (ARRAY[]::INT[])) AS inputs(value)",
            "2202E",
            "cannot accumulate empty arrays",
        ),
        (
            "SELECT array_agg(value) FROM (VALUES (ARRAY[1,2]::INT[]), (ARRAY[3,4,5]::INT[])) AS inputs(value)",
            "2202E",
            "cannot accumulate arrays of different dimensionality",
        ),
        (
            "SELECT array_agg(value) FROM (VALUES (array_fill(1, ARRAY[2], ARRAY[0])), (ARRAY[3,4])) AS inputs(value)",
            "2202E",
            "cannot accumulate arrays of different dimensionality",
        ),
    ];

    // Act
    let results = cases
        .iter()
        .map(|(sql, _, _)| fixture.execute(sql))
        .collect::<Vec<_>>();

    // Assert
    for ((sql, expected_state, expected_message), result) in cases.iter().zip(results) {
        let error = result.expect_err("invalid anyarray aggregate inputs are rejected");
        assert!(
            matches!(
                error,
                cassie::app::CassieError::Diagnostic(ref diagnostic)
                    if diagnostic.sql_state == *expected_state
                        && diagnostic.message == *expected_message
            ),
            "SQL {sql:?} must report {expected_state} / {expected_message}: {error:?}"
        );
    }
}

#[test]
fn should_filter_invalid_array_aggregate_inputs_before_validation() {
    // Arrange
    let fixture = sql_fixture("issue-811-filter-array-aggregate-input-shapes", &[]);

    // Act
    let result = fixture.execute(
        "SELECT array_agg(value) FILTER (WHERE selected)::TEXT \
         FROM (VALUES (NULL::INT[], FALSE), (ARRAY[1,2], TRUE)) AS inputs(value, selected)",
    );

    // Assert
    assert_eq!(
        result
            .expect("FILTER excludes a NULL array before ARRAY_AGG transition")
            .rows(),
        &[vec![Value::String("{{1,2}}".to_owned())]]
    );
}
