//! Native conditional type errors are binding errors, independent of row count.
use super::support_parameter_occurrence_wire::{run_script, Step};
use super::support_pgwire as wire;

fn cases(function: &str) -> Vec<(String, String, &'static str, String)> {
    let pairs = [
        ("n", "b", "integer", "boolean"),
        ("t", "b", "text", "boolean"),
        ("f", "t", "double precision", "text"),
        ("b", "n", "boolean", "integer"),
        ("b", "t", "boolean", "text"),
        ("t", "f", "text", "double precision"),
        ("s", "b", "smallint", "boolean"),
        ("l", "b", "bigint", "boolean"),
        ("b", "s", "boolean", "smallint"),
        ("b", "l", "boolean", "bigint"),
    ];
    let mut cases = pairs
        .into_iter()
        .map(|(left, right, left_type, right_type)| {
            let (state, message) = if function == "NULLIF" {
                (
                    "42883",
                    format!("operator does not exist: {left_type} = {right_type}"),
                )
            } else {
                (
                    "42804",
                    format!("{function} types {left_type} and {right_type} cannot be matched"),
                )
            };
            (
                format!("{function}({left},{right})"),
                format!("{function}($1::{left_type},$2::{right_type})"),
                state,
                message,
            )
        })
        .collect::<Vec<_>>();
    if function != "NULLIF" {
        cases.push((
            format!("{function}(n,f,b)"),
            format!("{function}($1::integer,$2::double precision,TRUE)"),
            "42804",
            format!("{function} types double precision and boolean cannot be matched"),
        ));
    }
    cases
}

fn assert_error(frames: &[(u8, Vec<u8>)], state: &str, message: &str) {
    assert_eq!(wire::error_code(frames).as_deref(), Some(state));
    let error = frames
        .iter()
        .find(|(tag, _)| *tag == b'E')
        .expect("binding error");
    assert!(
        wire::parse_error_fields(&error.1)
            .iter()
            .any(|(key, value)| *key == 'M' && value == message),
        "expected {message}"
    );
    assert!(!frames
        .iter()
        .any(|(tag, _)| matches!(tag, b'D' | b'C' | b'1')));
}

fn check_simple(function: &str) {
    let cases = cases(function);
    let mut steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(
        "CREATE TABLE records (n INT,b BOOLEAN,t TEXT,f FLOAT,s SMALLINT,l BIGINT)",
    )])];
    for populated in [false, true] {
        if populated {
            steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO records VALUES (1,TRUE,'x',2,3,4)",
            )]));
        }
        for (expression, _, _, _) in &cases {
            steps.push(Step::ThroughReady(vec![wire::simple_query_frame("BEGIN")]));
            steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
                &format!("SELECT {expression} FROM records"),
            )]));
            steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
                "ROLLBACK",
            )]));
        }
    }
    steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT count(*) FROM records",
    )]));
    let batches = run_script(&format!("conditional-types-{function}"), steps);
    assert_eq!(wire::error_code(&batches[0]), None);
    for offset in [1, 2 + cases.len() * 3] {
        for (index, (_, _, state, message)) in cases.iter().enumerate() {
            let frames = &batches[offset + index * 3 + 1];
            assert_error(frames, state, message);
            assert_eq!(
                frames.last().map(|frame| frame.1.as_slice()),
                Some(&b"E"[..])
            );
            assert_eq!(
                batches[offset + index * 3 + 2]
                    .last()
                    .map(|frame| frame.1.as_slice()),
                Some(&b"I"[..])
            );
        }
    }
    let recovery = batches.last().unwrap();
    assert_eq!(wire::error_code(recovery), None);
    assert_eq!(wire::data_rows(recovery), vec![vec![Some("1".into())]]);
}

fn check_parse(function: &str) {
    let cases = cases(function);
    let mut steps = Vec::new();
    for (index, (_, expression, _, _)) in cases.iter().enumerate() {
        steps.push(Step::ParseFlush(wire::parse_frame_with_types(
            &format!("invalid_{index}"),
            &format!("SELECT {expression}"),
            &[0, 0],
        )));
        steps.push(Step::ThroughReady(vec![wire::sync_frame()]));
    }
    steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT 7",
    )]));
    let batches = run_script(&format!("conditional-parse-{function}"), steps);
    for ((_, _, state, message), pair) in cases.iter().zip(batches.as_chunks::<2>().0) {
        assert_error(&pair[0], state, message);
        assert_eq!(
            pair[0].iter().map(|frame| frame.0).collect::<Vec<_>>(),
            vec![b'E']
        );
        assert_eq!(
            pair[1].last().map(|frame| frame.1.as_slice()),
            Some(&b"I"[..])
        );
    }
    assert_eq!(
        wire::data_rows(batches.last().unwrap()),
        vec![vec![Some("7".into())]]
    );
}

#[test]
fn should_reject_incompatible_greatest_given_empty_and_populated_input_when_types_are_resolved() {
    // Arrange
    let function = "GREATEST";
    // Act
    check_simple(function);
    // Assert
    // The helper checks native diagnostics, protocol state and recovery for every case.
}

#[test]
fn should_reject_incompatible_greatest_given_parameter_parse_when_types_are_resolved() {
    // Arrange
    let function = "GREATEST";
    // Act
    check_parse(function);
    // Assert
    // The helper checks native diagnostics, protocol state and recovery for every case.
}

#[test]
fn should_reject_incompatible_least_given_empty_and_populated_input_when_types_are_resolved() {
    // Arrange
    let function = "LEAST";
    // Act
    check_simple(function);
    // Assert
    // The helper checks native diagnostics, protocol state and recovery for every case.
}

#[test]
fn should_reject_incompatible_least_given_parameter_parse_when_types_are_resolved() {
    // Arrange
    let function = "LEAST";
    // Act
    check_parse(function);
    // Assert
    // The helper checks native diagnostics, protocol state and recovery for every case.
}

#[test]
fn should_reject_incompatible_nullif_given_empty_and_populated_input_when_types_are_resolved() {
    // Arrange
    let function = "NULLIF";
    // Act
    check_simple(function);
    // Assert
    // The helper checks native diagnostics, protocol state and recovery for every case.
}

#[test]
fn should_reject_incompatible_nullif_given_parameter_parse_when_types_are_resolved() {
    // Arrange
    let function = "NULLIF";
    // Act
    check_parse(function);
    // Assert
    // The helper checks native diagnostics, protocol state and recovery for every case.
}

#[test]
fn should_preserve_conditional_type_errors_given_nested_and_mutation_callers_when_input_is_empty() {
    // Arrange
    let mut cases = Vec::new();
    for function in ["GREATEST", "LEAST", "NULLIF"] {
        let expression = format!("{function}(n,b)");
        let (_, _, state, message) = self::cases(function).remove(0);
        let statements = [
            format!("SELECT SUM({expression}) FROM records"),
            format!("SELECT FIRST_VALUE({expression}) OVER () FROM records"),
            format!("WITH q AS (SELECT {expression} AS v FROM records) SELECT v FROM q"),
            format!("SELECT v FROM (SELECT {expression} AS v FROM records) AS q"),
            format!("SELECT n FROM records WHERE {expression} IS NULL"),
            format!("UPDATE records SET n={expression}"),
            format!("INSERT INTO records(n) SELECT {expression} FROM records"),
            format!("DELETE FROM records WHERE {expression} IS NULL"),
            format!("EXPLAIN SELECT {expression} FROM records"),
            format!("UPDATE records SET n=1 RETURNING {expression}"),
            format!("DELETE FROM records RETURNING {expression}"),
            format!("SELECT {expression} FROM records UNION ALL SELECT 1"),
            format!("SELECT n FROM records WHERE EXISTS (SELECT 1 WHERE {expression} IS NULL)"),
            format!("SELECT n FROM records ORDER BY {expression}"),
            format!("SELECT n FROM records GROUP BY n,b HAVING {expression} IS NULL"),
        ];
        cases.extend(
            statements
                .into_iter()
                .map(|sql| (sql, state, message.clone())),
        );
    }
    let mut steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(
        "CREATE TABLE records (n INT,b BOOLEAN)",
    )])];
    steps.extend(
        cases
            .iter()
            .map(|(sql, _, _)| Step::ThroughReady(vec![wire::simple_query_frame(sql)])),
    );
    steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT count(*) FROM records",
    )]));
    // Act
    let batches = run_script("conditional-type-callers", steps);
    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    for ((sql, state, message), frames) in cases.iter().zip(&batches[1..]) {
        assert_eq!(
            wire::error_code(frames).as_deref(),
            Some(*state),
            "{sql}: {frames:?}"
        );
        assert_error(frames, state, message);
        assert_eq!(
            frames.last().map(|frame| frame.1.as_slice()),
            Some(&b"I"[..])
        );
    }
    assert_eq!(
        wire::data_rows(batches.last().unwrap()),
        vec![vec![Some("0".into())]]
    );
}

#[test]
fn should_preserve_conditional_type_errors_given_declared_parameters_when_parse_resolves_signatures(
) {
    // Arrange
    let mut cases = Vec::new();
    let mut steps = Vec::new();
    for function in ["GREATEST", "LEAST", "NULLIF"] {
        for (oid, name) in [
            (21, "smallint"),
            (23, "integer"),
            (20, "bigint"),
            (701, "double precision"),
            (25, "text"),
        ] {
            for (oids, left, right) in [([oid, 16], name, "boolean"), ([16, oid], "boolean", name)]
            {
                let (state, message) = if function == "NULLIF" {
                    (
                        "42883",
                        format!("operator does not exist: {left} = {right}"),
                    )
                } else {
                    (
                        "42804",
                        format!("{function} types {left} and {right} cannot be matched"),
                    )
                };
                let statement = format!("declared_{}", cases.len());
                steps.push(Step::ParseFlush(wire::parse_frame_with_types(
                    &statement,
                    &format!("SELECT {function}($1,$2)"),
                    &oids,
                )));
                steps.push(Step::ThroughReady(vec![wire::sync_frame()]));
                cases.push((state, message));
            }
        }
    }
    steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT 7",
    )]));
    // Act
    let batches = run_script("conditional-declared-parameter-types", steps);
    // Assert
    for ((state, message), pair) in cases.iter().zip(batches.as_chunks::<2>().0) {
        assert_error(&pair[0], state, message);
        assert_eq!(
            pair[0].iter().map(|frame| frame.0).collect::<Vec<_>>(),
            vec![b'E']
        );
        assert_eq!(
            pair[1].last().map(|frame| frame.1.as_slice()),
            Some(&b"I"[..])
        );
    }
    assert_eq!(
        wire::data_rows(batches.last().unwrap()),
        vec![vec![Some("7".into())]]
    );
}
