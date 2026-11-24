#[test]
fn should_reject_declared_text_at_where_without_retagging_it_as_boolean() {
    // Arrange
    let mut cycles = Vec::new();
    for body in [Some(&b"true"[..]), None] {
        for format in [0, 1] {
            cycles.push(fixture::execute_cycle(
                "SELECT $1 AS flag WHERE $1",
                Some((25, 0, body)),
                format,
                true,
            ));
        }
    }

    // Act
    let batches = fixture::run_cycles("parameter-occurrence-known-text", &[], cycles);

    // Assert
    assert_eq!(batches.len(), 4);
    for frames in batches {
        assert_error(
            &frames,
            "42804",
            "argument of WHERE must be type boolean, not type text",
            None,
        );
        assert_eq!(tags(&frames), vec![b'E', b'Z']);
        assert_ready(&frames);
    }
}

#[test]
fn should_reject_an_unresolved_earlier_is_null_occurrence_before_parse_publication() {
    // Arrange
    let mut steps = Vec::new();
    for oid in [0, 705] {
        let name = format!("unresolved_null_{oid}");
        steps.push(Step::ParseFlush(wire::parse_frame_with_types(
            &name,
            "SELECT $1 IS NULL AS a, CAST($1 AS BOOLEAN) AS b",
            &[oid],
        )));
        steps.push(Step::ThroughReady(vec![
            wire::describe_statement_frame(&name),
            wire::sync_frame(),
        ]));
    }

    // Act
    let batches = run_script("parameter-occurrence-unresolved-is-null", steps);

    // Assert
    assert_eq!(batches.len(), 4);
    for pair in batches.as_chunks::<2>().0 {
        assert_error(
            &pair[0],
            "42P08",
            "could not determine data type of parameter $1",
            None,
        );
        assert_eq!(tags(&pair[0]), vec![b'E']);
        assert_eq!(tags(&pair[1]), vec![b'Z']);
        assert_ready(&pair[1]);
    }
}

#[test]
fn should_preserve_raw_invalid_boolean_text_in_parse_error() {
    // Arrange
    let cases = [
        ("quote_newline", "a\"b\n"),
        ("tab", "a\tb"),
        ("backslash", "a\\b"),
        ("unicode", "München 🚀"),
        ("control", "a\u{1}b"),
    ];
    let mut steps = Vec::new();
    for (label, input) in cases {
        let name = format!("invalid_boolean_raw_{label}");
        let sql = format!("SELECT $1 = '{input}' AS matched");
        steps.push(Step::ParseFlush(wire::parse_frame_with_types(
            &name,
            &sql,
            &[16],
        )));
        steps.push(Step::ThroughReady(vec![
            wire::describe_statement_frame(&name),
            wire::sync_frame(),
        ]));
    }

    // Act
    let batches = run_script("invalid-boolean-raw-error-text", steps);

    // Assert
    assert_eq!(batches.len(), cases.len() * 2);
    for ((_, input), pair) in cases.into_iter().zip(batches.as_chunks::<2>().0) {
        let message = format!("invalid input syntax for type boolean: \"{input}\"");
        assert_error(&pair[0], "22P02", &message, None);
        assert_eq!(tags(&pair[0]), vec![b'E']);
        assert_eq!(tags(&pair[1]), vec![b'Z']);
        assert_ready(&pair[1]);
    }
}
