#[test]
fn should_reject_declared_non_boolean_searched_case_conditions_during_parse() {
    // Arrange
    let cases = [(25, "text"), (23, "integer")];
    let mut steps = Vec::new();
    for (oid, _) in cases {
        let name = format!("searched_case_{oid}");
        steps.push(Step::ParseFlush(wire::parse_frame_with_types(
            &name,
            "SELECT CASE WHEN $1 THEN 1 ELSE 2 END",
            &[oid],
        )));
        steps.push(Step::ThroughReady(vec![
            wire::describe_statement_frame(&name),
            wire::sync_frame(),
        ]));
    }

    // Act
    let batches = run_script("searched-case-boolean-input", steps);

    // Assert
    assert_eq!(batches.len(), cases.len() * 2);
    for ((_, type_name), pair) in cases.into_iter().zip(batches.as_chunks::<2>().0) {
        assert_error(
            &pair[0],
            "42804",
            &format!("argument of CASE/WHEN must be type boolean, not type {type_name}"),
            None,
        );
        assert_eq!(tags(&pair[0]), vec![b'E']);
        assert_eq!(tags(&pair[1]), vec![b'Z']);
        assert_ready(&pair[1]);
    }
}

#[test]
fn should_reject_non_boolean_parameters_in_later_searched_case_conditions() {
    // Arrange
    let cases = [(25, "text"), (23, "integer")];
    let mut steps = Vec::new();
    for (oid, _) in cases {
        let name = format!("searched_case_later_{oid}");
        steps.push(Step::ParseFlush(wire::parse_frame_with_types(
            &name,
            "SELECT CASE WHEN TRUE THEN 1 WHEN $1 THEN 2 ELSE 3 END",
            &[oid],
        )));
        steps.push(Step::ThroughReady(vec![
            wire::describe_statement_frame(&name),
            wire::sync_frame(),
        ]));
    }

    // Act
    let batches = run_script("searched-case-later-boolean-input", steps);

    // Assert
    assert_eq!(batches.len(), cases.len() * 2);
    for ((_, type_name), pair) in cases.into_iter().zip(batches.as_chunks::<2>().0) {
        assert_error(
            &pair[0],
            "42804",
            &format!("argument of CASE/WHEN must be type boolean, not type {type_name}"),
            None,
        );
        assert_eq!(tags(&pair[0]), vec![b'E']);
        assert_eq!(tags(&pair[1]), vec![b'Z']);
        assert_ready(&pair[1]);
    }
}
