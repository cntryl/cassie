#[test]
fn should_reject_text_conjunction_operands_during_parse_in_either_order() {
    // Arrange
    let statements = [
        "SELECT $1 AND CAST($1 AS TEXT) AS result",
        "SELECT CAST($1 AS TEXT) AND $1 AS result",
    ];
    let mut steps = Vec::new();
    for (index, sql) in statements.into_iter().enumerate() {
        for oid in [0, 705, 16, 25] {
            let name = format!("and_text_{index}_{oid}");
            steps.push(Step::ParseFlush(wire::parse_frame_with_types(
                &name,
                sql,
                &[oid],
            )));
            steps.push(Step::ThroughReady(vec![
                wire::describe_statement_frame(&name),
                wire::sync_frame(),
            ]));
        }
    }

    // Act
    let batches = run_script("parameter-occurrence-and-order", steps);

    // Assert
    assert_eq!(batches.len(), 16);
    for pair in batches.as_chunks::<2>().0 {
        assert_error(
            &pair[0],
            "42804",
            "argument of AND must be type boolean, not type text",
            None,
        );
        assert_eq!(tags(&pair[0]), vec![b'E']);
        assert_eq!(tags(&pair[1]), vec![b'Z']);
        assert_ready(&pair[1]);
    }
}
