#[test]
fn should_reject_earlier_bare_unknown_occurrences_before_parse_publication() {
    // Arrange
    let cases = [
        ("SELECT $1 AS flag WHERE $1", "boolean versus text"),
        (
            "SELECT $1 AS a, CAST($1 AS BOOLEAN) AS b",
            "boolean versus text",
        ),
        ("SELECT $1 AS a, NOT $1 AS b", "boolean versus text"),
        (
            "SELECT $1 AS a, CAST($1 AS INT) AS b",
            "integer versus text",
        ),
        ("SELECT $1 AS value LIMIT $1", "bigint versus text"),
        (
            "SELECT $1 AS a, $1 AS b, NOT $1 AS c",
            "boolean versus text",
        ),
        (
            "SELECT $1 AS outer_value, EXISTS (SELECT 1 WHERE $1) AS gate",
            "boolean versus text",
        ),
        ("EXPLAIN SELECT $1 AS flag WHERE $1", "boolean versus text"),
    ];
    let mut steps = Vec::new();
    for (index, (sql, _)) in cases.iter().enumerate() {
        for oid in [0, 705] {
            let name = format!("rejected_{index}_{oid}");
            steps.push(Step::ParseFlush(wire::parse_frame_with_types(
                &name,
                sql,
                &[oid],
            )));
            // Describe is sent only after the observed Parse reply. A rejected
            // Parse must ignore it until Sync rather than publish metadata.
            steps.push(Step::ThroughReady(vec![
                wire::describe_statement_frame(&name),
                wire::sync_frame(),
            ]));
        }
    }

    // Act
    let batches = run_script("parameter-occurrence-rejection", steps);

    // Assert
    assert_eq!(batches.len(), cases.len() * 4);
    for ((_, detail), pair_of_cases) in cases.into_iter().zip(batches.as_chunks::<4>().0) {
        for pair in pair_of_cases.as_chunks::<2>().0 {
            assert_eq!(pair[0].len(), 1, "one reply before any Sync");
            assert_error(
                &pair[0],
                "42P08",
                "inconsistent types deduced for parameter $1",
                Some(detail),
            );
            assert_eq!(tags(&pair[0]), vec![b'E']);
            assert_eq!(tags(&pair[1]), vec![b'Z']);
            assert_ready(&pair[1]);
        }
    }
}
