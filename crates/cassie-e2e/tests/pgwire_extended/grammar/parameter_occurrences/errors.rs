#[test]
fn should_reject_invalid_boolean_literals_during_parse() {
    // Arrange
    let cases = [
        (
            "SELECT $1 = 'maybe' AS matched",
            "invalid_bool_left_parameter",
        ),
        (
            "SELECT 'maybe' = $1 AS matched",
            "invalid_bool_right_parameter",
        ),
        (
            "SELECT $1 BETWEEN 'maybe' AND 'true' AS matched",
            "invalid_bool_between",
        ),
        ("SELECT $1 IN ('maybe') AS matched", "invalid_bool_in_list"),
        (
            "SELECT CAST($1 AS BOOLEAN) = 'maybe' AS matched",
            "invalid_bool_cast_parameter",
        ),
        (
            "SELECT $1 IS DISTINCT FROM 'maybe' AS matched",
            "invalid_bool_distinct_parameter",
        ),
        (
            "SELECT $1 AND 'maybe' AS matched",
            "invalid_bool_and_parameter",
        ),
        (
            "SELECT 'maybe' OR $1 AS matched",
            "invalid_bool_or_parameter",
        ),
        (
            "SELECT CASE WHEN 'maybe' THEN $1 ELSE $1 END AS matched",
            "invalid_bool_case_condition",
        ),
        (
            "SELECT NOT 'maybe' AS invalid, $1 AS marker",
            "invalid_bool_not_literal",
        ),
        (
            "SELECT CAST('maybe' AS BOOLEAN) AS invalid, $1 AS marker",
            "invalid_bool_cast_literal",
        ),
        (
            "SELECT COALESCE('maybe', $1) AS invalid",
            "invalid_bool_coalesce_literal",
        ),
        (
            "SELECT COALESCE('maybe', CAST($1 AS BOOLEAN)) AS invalid",
            "invalid_bool_coalesce_cast_literal",
        ),
        (
            "SELECT CASE WHEN $1 THEN 'maybe' ELSE TRUE END AS invalid",
            "invalid_bool_case_result",
        ),
        (
            "SELECT CASE $1 WHEN 'maybe' THEN TRUE ELSE FALSE END AS invalid",
            "invalid_bool_simple_case_operand",
        ),
        (
            "SELECT TRUE = 'maybe' AS invalid, $1 AS marker",
            "invalid_bool_literal_without_local_parameter",
        ),
    ];
    let mut steps = Vec::new();
    for (sql, name) in cases {
        steps.push(Step::ParseFlush(wire::parse_frame_with_types(
            name,
            sql,
            &[16],
        )));
        steps.push(Step::ThroughReady(vec![
            wire::describe_statement_frame(name),
            wire::sync_frame(),
        ]));
    }

    // Act
    let batches = run_script("invalid-boolean-comparison-literal", steps);

    // Assert
    assert_eq!(batches.len(), cases.len() * 2);
    for ((sql, name), pair) in cases.iter().zip(batches.as_chunks::<2>().0) {
        assert_eq!(tags(&pair[0]), vec![b'E'], "{name}: {sql}");
        assert_error(
            &pair[0],
            "22P02",
            "invalid input syntax for type boolean: \"maybe\"",
            None,
        );
        assert_eq!(tags(&pair[1]), vec![b'Z']);
        assert_ready(&pair[1]);
    }
}

#[test]
fn should_reject_invalid_boolean_cast_during_parse_without_parameters() {
    // Arrange
    let mut steps = vec![Step::ParseFlush(wire::parse_frame_with_types(
        "invalid_cast_without_parameters",
        "SELECT CAST('maybe' AS BOOLEAN) AS invalid",
        &[],
    ))];
    steps.push(Step::ThroughReady(vec![wire::sync_frame()]));

    // Act
    let batches = run_script("invalid-boolean-cast-without-parameters", steps);

    // Assert
    assert_eq!(batches.len(), 2);
    assert_error(
        &batches[0],
        "22P02",
        "invalid input syntax for type boolean: \"maybe\"",
        None,
    );
    assert_eq!(tags(&batches[0]), vec![b'E']);
    assert_eq!(tags(&batches[1]), vec![b'Z']);
    assert_ready(&batches[1]);
}
