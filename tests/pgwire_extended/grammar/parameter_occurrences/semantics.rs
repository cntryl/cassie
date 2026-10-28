#[test]
fn should_preserve_mixed_explicit_cast_outputs_after_occurrence_resolution() {
    // Arrange
    let cases = [
        PairCase {
            sql: "SELECT CAST($1 AS BOOLEAN) AS a, $1 AS b",
            input: b"true",
            parameter_oid: 16,
            columns: [(16, 1), (16, 1)],
            text: [b"t", b"t"],
            binary: [&[1], &[1]],
        },
        PairCase {
            sql: "SELECT NOT $1 AS a, $1 AS b",
            input: b"true",
            parameter_oid: 16,
            columns: [(16, 1), (16, 1)],
            text: [b"f", b"t"],
            binary: [&[0], &[1]],
        },
        PairCase {
            sql: "SELECT CAST($1 AS INT) AS a, $1 AS b",
            input: b"1",
            parameter_oid: 23,
            columns: [(23, 4), (23, 4)],
            text: [b"1", b"1"],
            binary: [&[0, 0, 0, 1], &[0, 0, 0, 1]],
        },
        PairCase {
            sql: "SELECT CAST($1 AS BIGINT) AS a, $1 AS b",
            input: b"1",
            parameter_oid: 20,
            columns: [(20, 8), (20, 8)],
            text: [b"1", b"1"],
            binary: [&[0, 0, 0, 0, 0, 0, 0, 1], &[0, 0, 0, 0, 0, 0, 0, 1]],
        },
        PairCase {
            sql: "SELECT NOT $1 AS a, CAST($1 AS BOOLEAN) AS b",
            input: b"true",
            parameter_oid: 16,
            columns: [(16, 1), (16, 1)],
            text: [b"f", b"t"],
            binary: [&[0], &[1]],
        },
        PairCase {
            sql: "SELECT CAST($1 AS BOOLEAN) AS a, CAST($1 AS INT) AS b",
            input: b"true",
            parameter_oid: 16,
            columns: [(16, 1), (23, 4)],
            text: [b"t", b"1"],
            binary: [&[1], &[0, 0, 0, 1]],
        },
        PairCase {
            sql: "SELECT CAST($1 AS BOOLEAN) AS a, $1 IS NULL AS b",
            input: b"true",
            parameter_oid: 16,
            columns: [(16, 1), (16, 1)],
            text: [b"t", b"f"],
            binary: [&[1], &[0]],
        },
    ];
    let mut requests = Vec::new();
    let mut cycles = Vec::new();
    for (index, case) in cases.iter().enumerate() {
        for oid in [0, 705] {
            for format in [0, 1] {
                requests.push((index, oid, format));
                cycles.push(fixture::execute_cycle(
                    case.sql,
                    Some((oid, 0, Some(case.input))),
                    format,
                    true,
                ));
            }
        }
    }

    // Act
    let batches = fixture::run_cycles("parameter-occurrence-ordered-casts", &[], cycles);

    // Assert
    assert_eq!(batches.len(), requests.len());
    for ((index, oid, format), frames) in requests.into_iter().zip(batches) {
        let case = &cases[index];
        fixture::assert_success(&frames);
        assert_parameter_oids(&frames, &[case.parameter_oid]);
        assert_descriptors(&frames, &case.columns, &["a", "b"], format, true);
        let fields = if format == 0 { case.text } else { case.binary };
        assert_eq!(
            fixture::data_row_payloads(&frames),
            vec![row_payload(&fields)],
            "{}, supplied OID {oid}, result format {format}",
            case.sql
        );
        assert_command(&frames, b"SELECT 1\0");
    }
}

#[test]
fn should_keep_valid_boolean_literals_parseable() {
    // Arrange
    let cases = [
        ("SELECT $1 = 'yes' AS matched", "valid_boolean", 16),
        ("SELECT $1 AND 'yes' AS matched", "valid_boolean_and", 16),
        (
            "SELECT CAST('yes' AS BOOLEAN) AS flag, $1 AS marker",
            "valid_boolean_cast",
            16,
        ),
        (
            "SELECT COALESCE('yes', $1) AS flag",
            "valid_boolean_coalesce",
            16,
        ),
        (
            "SELECT CASE WHEN 'yes' THEN $1 ELSE $1 END AS flag",
            "valid_boolean_case_condition",
            16,
        ),
        (
            "SELECT CASE WHEN $1 THEN 'yes' ELSE TRUE END AS flag",
            "valid_boolean_case_result",
            16,
        ),
        (
            "SELECT CASE $1 WHEN 'yes' THEN TRUE ELSE FALSE END AS flag",
            "valid_boolean_simple_case",
            16,
        ),
    ];
    let mut steps = Vec::new();
    for (sql, name, oid) in cases {
        steps.push(Step::ParseFlush(wire::parse_frame_with_types(
            name,
            sql,
            &[oid],
        )));
        steps.push(Step::ThroughReady(vec![wire::sync_frame()]));
    }

    // Act
    let batches = run_script("valid-boolean-comparison-literals", steps);

    // Assert
    assert_eq!(batches.len(), cases.len() * 2);
    for (index, pair) in batches.as_chunks::<2>().0.iter().enumerate() {
        assert_eq!(
            tags(&pair[0]),
            vec![b'1'],
            "{}: {:?}",
            cases[index].0,
            pair[0]
        );
        assert_eq!(tags(&pair[1]), vec![b'Z']);
        assert_ready(&pair[1]);
    }
}

fn assert_typed_boolean_case_result(sql: &str, expected: &[u8]) {
    // Arrange
    let cycle = fixture::execute_cycle(sql, Some((16, 0, Some(b"false"))), 0, true);

    // Act
    let batches = fixture::run_cycles("boolean-case-typed-parameter", &[], vec![cycle]);

    // Assert
    assert_eq!(batches.len(), 1);
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_parameter_oids(frames, &[16]);
    assert_descriptors(frames, &[(16, 1)], &["flag"], 0, true);
    assert_eq!(fixture::data_row_payloads(frames), vec![fixture::one_field_row(expected)]);
    assert_command(frames, b"SELECT 1\0");
    assert!(frames.iter().any(|(tag, _)| *tag == b'T'), "{sql}");
}

#[test]
fn should_infer_boolean_case_result_when_parameter_branch_is_selected() {
    // Arrange
    // Act
    // Assert
    assert_typed_boolean_case_result(
        "SELECT CASE WHEN TRUE THEN $1 ELSE 'yes' END AS flag",
        b"f",
    );
}

#[test]
fn should_infer_boolean_case_result_when_literal_branch_is_selected() {
    // Arrange
    // Act
    // Assert
    assert_typed_boolean_case_result(
        "SELECT CASE WHEN FALSE THEN $1 ELSE 'yes' END AS flag",
        b"t",
    );
}
