#[test]
fn should_reject_declared_non_boolean_join_parameters_during_parse() {
    // Arrange
    let cases = [
        (
            "SELECT bool_wire.id FROM bool_wire JOIN bool_wire_empty ON $1",
            "join_text",
            25,
            "text",
        ),
        (
            "SELECT bool_wire.id FROM bool_wire JOIN bool_wire_empty ON $1",
            "join_integer",
            23,
            "integer",
        ),
    ];
    let mut steps = Vec::new();
    for (sql, name, oid, _) in cases {
        steps.push(Step::ParseFlush(wire::parse_frame_with_types(
            name,
            sql,
            &[oid],
        )));
        steps.push(Step::ThroughReady(vec![
            wire::describe_statement_frame(name),
            wire::sync_frame(),
        ]));
    }

    // Act
    let batches = run_script("join-boolean-input", steps);

    // Assert
    assert_eq!(batches.len(), cases.len() * 2);
    for ((_, _, _, type_name), pair) in cases.into_iter().zip(batches.as_chunks::<2>().0) {
        assert_error(
            &pair[0],
            "42804",
            &format!("argument of JOIN/ON must be type boolean, not type {type_name}"),
            None,
        );
        assert_eq!(tags(&pair[0]), vec![b'E']);
        assert_eq!(tags(&pair[1]), vec![b'Z']);
        assert_ready(&pair[1]);
    }
}

#[test]
fn should_reject_declared_non_boolean_having_parameters_during_parse() {
    // Arrange
    let cases = [
        ("having_text", 25, "text"),
        ("having_integer", 23, "integer"),
    ];
    let mut steps = Vec::new();
    for (name, oid, _) in cases {
        steps.push(Step::ParseFlush(wire::parse_frame_with_types(
            name,
            "SELECT COUNT(*) FROM bool_wire HAVING $1",
            &[oid],
        )));
        steps.push(Step::ThroughReady(vec![
            wire::describe_statement_frame(name),
            wire::sync_frame(),
        ]));
    }

    // Act
    let batches = run_script("having-boolean-input", steps);

    // Assert
    assert_eq!(batches.len(), cases.len() * 2);
    for ((_, _, type_name), pair) in cases.into_iter().zip(batches.as_chunks::<2>().0) {
        assert_error(
            &pair[0],
            "42804",
            &format!("argument of HAVING must be type boolean, not type {type_name}"),
            None,
        );
        assert_eq!(tags(&pair[0]), vec![b'E']);
        assert_eq!(tags(&pair[1]), vec![b'Z']);
        assert_ready(&pair[1]);
    }
}
