//! PostgreSQL 18 Boolean receive semantics, independent packet literals and legacy adapters.
use super::support_pgwire as wire;
use super::support_type_metadata_contract as fixture;

#[test]
fn should_normalize_every_boolean_binary_byte_with_declared_metadata() {
    // Arrange
    let cases: Vec<_> = (0..=u8::MAX)
        .flat_map(|byte| [0, 1].map(|format| (byte, format)))
        .collect();
    let cycles = cases
        .iter()
        .map(|(byte, format)| {
            fixture::execute_cycle(
                "SELECT $1 AS flag",
                Some((16, 1, Some(&[*byte]))),
                *format,
                true,
            )
        })
        .collect();

    // Act
    let batches = fixture::run_cycles("boolean-all-binary-bytes", &[], cycles);

    // Assert
    assert_eq!(batches.len(), cases.len());
    for ((byte, format), frames) in cases.into_iter().zip(batches) {
        fixture::assert_success(&frames);
        fixture::assert_single_column_descriptors(&frames, (16, 1, -1), format, true);
        let parameter = frames
            .iter()
            .find(|(tag, _)| *tag == b't')
            .expect("parameter metadata");
        assert_eq!(wire::parse_parameter_description(&parameter.1), vec![16]);
        let field = if format == 0 {
            vec![if byte == 0 { b'f' } else { b't' }]
        } else {
            vec![u8::from(byte != 0)]
        };
        assert_eq!(
            fixture::data_row_payloads(&frames),
            vec![fixture::one_field_row(&field)],
            "input {byte}, format {format}"
        );
        let command = frames
            .iter()
            .find(|(tag, _)| *tag == b'C')
            .expect("command completion");
        assert_eq!(command.1, b"SELECT 1\0");
    }
}

#[test]
fn should_recover_from_boolean_binary_width_errors_after_sync() {
    // Arrange
    let cases = [
        (&[][..], "08P01"),
        (&[0, 0][..], "22P03"),
        (&[255, 0][..], "22P03"),
    ];
    let mut cycles = Vec::new();
    for (input, _) in cases {
        // LIMIT0 still validates Bind, and the pending Execute must be ignored after its error.
        cycles.push(fixture::execute_cycle(
            "SELECT $1 AS flag LIMIT 0",
            Some((16, 1, Some(input))),
            1,
            true,
        ));
        cycles.push(fixture::execute_cycle(
            "SELECT $1 AS flag",
            Some((16, 1, Some(&[1]))),
            1,
            true,
        ));
    }

    // Act
    let batches = fixture::run_cycles("boolean-binary-length-recovery", &[], cycles);

    // Assert
    assert_eq!(batches.len(), cases.len() * 2);
    for ((_, state), pair) in cases.into_iter().zip(batches.as_chunks::<2>().0) {
        let rejected = &pair[0];
        assert_eq!(wire::error_code(rejected).as_deref(), Some(state));
        assert_eq!(rejected.iter().filter(|(tag, _)| *tag == b'E').count(), 1);
        assert!(!rejected
            .iter()
            .any(|(tag, _)| matches!(*tag, b'2' | b'D' | b'C')));
        assert_eq!(
            rejected.last().map(|frame| frame.1.as_slice()),
            Some(&b"I"[..])
        );
        fixture::assert_success(&pair[1]);
        assert_eq!(
            fixture::data_row_payloads(&pair[1]),
            vec![fixture::one_field_row(&[1])]
        );
    }
}

#[test]
fn should_normalize_contextual_boolean_binary_parameters() {
    // Arrange
    let cases = [
        (0, "SELECT CAST($1 AS BOOLEAN) AS flag WHERE $1", true, 1),
        (705, "SELECT CAST($1 AS BOOLEAN) AS flag WHERE $1", true, 1),
        (0, "SELECT NOT $1 AS flag", true, 0),
        (705, "SELECT NOT $1 AS flag", true, 0),
        (16, "SELECT $1 AS flag WHERE $1", true, 1),
        (16, "SELECT $1 AS flag LIMIT 0", false, 1),
        (16, "SELECT $1 AS flag WHERE FALSE", false, 1),
    ];
    let cycles = cases
        .map(|(oid, sql, _, _)| fixture::execute_cycle(sql, Some((oid, 1, Some(&[128]))), 1, true))
        .to_vec();

    // Act
    let batches = fixture::run_cycles("boolean-inferred-binary-bind", &[], cycles);

    // Assert
    for ((_, _, populated, byte), frames) in cases.into_iter().zip(batches) {
        fixture::assert_success(&frames);
        fixture::assert_single_column_descriptors(&frames, (16, 1, -1), 1, true);
        let parameter = frames
            .iter()
            .find(|(tag, _)| *tag == b't')
            .expect("inferred parameter metadata");
        assert_eq!(wire::parse_parameter_description(&parameter.1), vec![16]);
        let expected = if populated {
            vec![fixture::one_field_row(&[byte])]
        } else {
            vec![]
        };
        assert_eq!(fixture::data_row_payloads(&frames), expected);
    }
}

#[test]
fn should_normalize_boolean_elements_without_changing_legacy_array_identity() {
    // Arrange
    // Existing v2 array adapter:34016, not PostgreSQL bool[]1000. No common array-OID claim.
    let mut input = Vec::new();
    for header in [1_i32, 1, 16, 3, 1, 1] {
        input.extend_from_slice(&header.to_be_bytes());
    }
    input.push(255);
    input.extend_from_slice(&(-1_i32).to_be_bytes());
    input.extend_from_slice(&1_i32.to_be_bytes());
    input.push(128);
    let mut expected = input.clone();
    expected[24] = 1;
    expected[33] = 1;
    let cycles = [0, 1]
        .map(|format| {
            fixture::execute_cycle(
                "SELECT $1 AS flags",
                Some((34016, 1, Some(&input))),
                format,
                true,
            )
        })
        .to_vec();

    // Act
    let batches = fixture::run_cycles("boolean-legacy-array-binary", &[], cycles);

    // Assert
    for (format, frames) in [0, 1].into_iter().zip(batches) {
        fixture::assert_success(&frames);
        fixture::assert_single_column_descriptors(&frames, (34016, -1, -1), format, true);
        let field = if format == 0 {
            b"[true,null,true]".as_slice()
        } else {
            &expected
        };
        assert_eq!(
            fixture::data_row_payloads(&frames),
            vec![fixture::one_field_row(field)]
        );
    }
}

#[test]
fn should_preserve_named_boolean_statement_metadata_across_rebindings() {
    // Arrange
    let cases = [
        (Some(vec![255_u8]), 0, None),
        (Some(vec![]), 1, Some("08P01")),
        (Some(vec![128]), 1, None),
        (Some(vec![0, 0]), 1, Some("22P03")),
        (None, 1, None),
        (Some(vec![0]), 0, None),
    ];
    let mut cycles = Vec::new();
    for (index, (body, format, _)) in cases.iter().enumerate() {
        let mut cycle = Vec::new();
        if index == 0 {
            cycle.push(wire::parse_frame_with_types(
                "shared_bool",
                "SELECT $1 AS flag",
                &[16],
            ));
        }
        cycle.extend([
            wire::describe_statement_frame("shared_bool"),
            wire::bind_frame_with_formats("", "shared_bool", &[1], &[body.as_deref()], &[*format]),
            wire::describe_portal_frame(""),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]);
        cycles.push(cycle);
    }

    // Act
    let batches = fixture::run_cycles("boolean-named-statement-rebindings", &[], cycles);

    // Assert
    assert_eq!(batches.len(), cases.len());
    for ((body, format, state), frames) in cases.into_iter().zip(batches) {
        if let Some(state) = state {
            assert_eq!(wire::error_code(&frames).as_deref(), Some(state));
            assert!(!frames
                .iter()
                .any(|(tag, _)| matches!(*tag, b'2' | b'D' | b'C')));
            assert_eq!(
                frames.last().map(|frame| frame.1.as_slice()),
                Some(&b"I"[..])
            );
            continue;
        }
        fixture::assert_success(&frames);
        fixture::assert_single_column_descriptors(&frames, (16, 1, -1), format, true);
        let parameter = frames
            .iter()
            .find(|(tag, _)| *tag == b't')
            .expect("named parameter metadata");
        assert_eq!(wire::parse_parameter_description(&parameter.1), vec![16]);
        let expected = body.map_or_else(
            || vec![0, 1, 255, 255, 255, 255],
            |body| {
                assert_eq!(body.len(), 1);
                let byte = body[0];
                let field = if format == 0 {
                    vec![if byte == 0 { b'f' } else { b't' }]
                } else {
                    vec![u8::from(byte != 0)]
                };
                fixture::one_field_row(&field)
            },
        );
        assert_eq!(fixture::data_row_payloads(&frames), vec![expected]);
    }
}

#[test]
fn should_persist_boolean_binary_normalization_across_recovery_boundaries() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE bool_registry (id INT PRIMARY KEY, flag BOOLEAN)",
            vec![],
        ),
        (
            "CREATE INDEX bool_registry_flags ON bool_registry(flag)",
            vec![],
        ),
    ];
    let simple = |sql| vec![wire::simple_query_frame(sql)];
    let binary = |sql, body: &[u8]| fixture::execute_cycle(sql, Some((16, 1, Some(body))), 1, true);
    let stages = vec![
        vec![
            binary(
                "INSERT INTO bool_registry (id, flag) VALUES (1, $1) RETURNING flag",
                &[255],
            ),
            simple("BEGIN"),
            simple("SAVEPOINT bool_save"),
            binary(
                "UPDATE bool_registry SET flag = $1 WHERE id = 1 RETURNING flag",
                &[0],
            ),
            simple("ROLLBACK TO SAVEPOINT bool_save"),
            binary(
                "UPDATE bool_registry SET flag = $1 WHERE id = 1 RETURNING flag",
                &[0, 0],
            ),
            simple("ROLLBACK TO SAVEPOINT bool_save"),
            binary(
                "INSERT INTO bool_registry (id, flag) VALUES (2, $1) RETURNING flag",
                &[128],
            ),
            simple("COMMIT"),
            binary(
                "SELECT flag FROM bool_registry WHERE flag = $1 ORDER BY id",
                &[255],
            ),
        ],
        vec![
            binary(
                "SELECT flag FROM bool_registry WHERE flag = $1 ORDER BY id",
                &[128],
            ),
            simple("BEGIN"),
            binary(
                "UPDATE bool_registry SET flag = $1 WHERE id = 1 RETURNING flag",
                &[0],
            ),
            simple("ROLLBACK"),
            binary(
                "SELECT flag FROM bool_registry WHERE id = 1 AND flag = $1",
                &[255],
            ),
        ],
    ];

    // Act
    let stages =
        fixture::run_cycles_across_restart("boolean-binary-savepoint-restart", &setup, stages);

    // Assert
    assert_eq!(stages.len(), 2);
    assert_eq!(stages[0].len(), 10);
    assert_eq!(stages[1].len(), 5);
    for (stage, states) in stages
        .iter()
        .zip([b"ITTTTETTII".as_slice(), b"ITTII".as_slice()])
    {
        assert_eq!(stage.len(), states.len());
        for (frames, state) in stage.iter().zip(states.iter().copied()) {
            assert_eq!(
                frames.last().map(|frame| frame.1.as_slice()),
                Some(&[state][..])
            );
        }
    }
    for (stage, index, fields) in [
        (0, 0, vec![1]),
        (0, 3, vec![0]),
        (0, 7, vec![1]),
        (0, 9, vec![1, 1]),
        (1, 0, vec![1, 1]),
        (1, 2, vec![0]),
        (1, 4, vec![1]),
    ] {
        let frames = &stages[stage][index];
        assert_eq!(wire::error_code(frames), None);
        fixture::assert_single_column_descriptors(frames, (16, 1, -1), 1, true);
        let expected: Vec<_> = fields
            .into_iter()
            .map(|field| fixture::one_field_row(&[field]))
            .collect();
        assert_eq!(
            fixture::data_row_payloads(frames),
            expected,
            "stage {stage}, cycle {index}"
        );
    }
    assert_eq!(wire::error_code(&stages[0][5]).as_deref(), Some("22P03"));
    assert_eq!(
        fixture::data_row_payloads(&stages[0][5]),
        Vec::<Vec<u8>>::new()
    );
}
