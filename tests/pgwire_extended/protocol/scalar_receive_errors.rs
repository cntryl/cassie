//! Existing fixed-scalar framing, semantic error priority and durable recovery.
use super::support_pgwire as wire;
use super::support_scalar_receive as receive;
use super::support_type_codec_matrix as matrix;
use super::support_type_metadata_contract as fixture;
use receive::{Expected, Outcome};

#[test]
fn should_preserve_fixed_scalar_metadata_after_binary_receive_errors() {
    // Arrange
    let mut cycles = Vec::new();
    let mut expected = Vec::new();
    for case in matrix::SCALAR_CASES.iter().filter(|case| case.typlen > 0) {
        for format in [0, 1] {
            let statement = format!("fixed_{}_{}", case.oid, format);
            let mut excess = case.input_binary.to_vec();
            excess.push(0);
            let mut inputs = vec![(Some(Vec::new()), Outcome::Error("08P01"))];
            if case.input_binary.len() > 1 {
                inputs.push((
                    Some(case.input_binary[..case.input_binary.len() - 1].to_vec()),
                    Outcome::Error("08P01"),
                ));
            }
            inputs.push((Some(excess), Outcome::Error("22P03")));
            for (index, (body, outcome)) in inputs.into_iter().enumerate() {
                cycles.push(receive::named_cycle(
                    &statement,
                    (index == 0).then_some(("SELECT $1 AS v", case.oid)),
                    body.as_deref(),
                    format,
                ));
                expected.push(Expected {
                    case,
                    format,
                    outcome,
                });
                cycles.push(receive::named_cycle(
                    &statement,
                    None,
                    Some(case.input_binary),
                    format,
                ));
                expected.push(Expected {
                    case,
                    format,
                    outcome: Outcome::Value,
                });
            }
            cycles.push(receive::named_cycle(&statement, None, None, format));
            expected.push(Expected {
                case,
                format,
                outcome: Outcome::Null,
            });
        }
    }

    // Act
    let batches = fixture::run_cycles("scalar-receive-width-rebinding", &[], cycles);

    // Assert
    assert_eq!(batches.len(), 122);
    receive::assert_batches(&batches, &expected);
}

#[test]
fn should_prioritize_temporal_range_errors_over_excess_binary_data() {
    // Arrange
    let invalid = receive::invalid_temporal_cases();
    let mut cycles = Vec::new();
    let mut expected = Vec::new();
    for (index, (oid, body)) in invalid.into_iter().enumerate() {
        let case = matrix::SCALAR_CASES
            .iter()
            .find(|case| case.oid == oid)
            .expect("temporal fixture");
        let mut excess = body.clone();
        excess.push(0);
        let short = body[..body.len() - 1].to_vec();
        for format in [0, 1] {
            let statement = format!("range_{index}_{format}");
            for (position, (input, state)) in
                [(&body, "22008"), (&excess, "22008"), (&short, "08P01")]
                    .into_iter()
                    .enumerate()
            {
                cycles.push(receive::named_cycle(
                    &statement,
                    (position == 0).then_some(("SELECT $1 AS v", oid)),
                    Some(input),
                    format,
                ));
                expected.push(Expected {
                    case,
                    format,
                    outcome: Outcome::Error(state),
                });
                cycles.push(receive::named_cycle(
                    &statement,
                    None,
                    Some(case.input_binary),
                    format,
                ));
                expected.push(Expected {
                    case,
                    format,
                    outcome: Outcome::Value,
                });
            }
            cycles.push(receive::named_cycle(&statement, None, None, format));
            expected.push(Expected {
                case,
                format,
                outcome: Outcome::Null,
            });
        }
    }

    // Act
    let batches = fixture::run_cycles("scalar-temporal-receive-priority", &[], cycles);

    // Assert
    assert_eq!(batches.len(), 56);
    receive::assert_batches(&batches, &expected);
}

#[test]
fn should_preserve_scalar_rows_after_rejected_binary_mutations() {
    // Arrange
    let cases: Vec<_> = matrix::SCALAR_CASES
        .iter()
        .filter(|case| case.typlen > 1)
        .collect();
    let setup: Vec<_> = cases
        .iter()
        .flat_map(|case| {
            [
                format!(
                    "CREATE TABLE receive_{} (k INT PRIMARY KEY, v {})",
                    case.name, case.sql_type
                ),
                format!(
                    "CREATE INDEX receive_{}_v ON receive_{}(v)",
                    case.name, case.name
                ),
            ]
        })
        .collect();
    let mut initial = Vec::new();
    let mut restarted = Vec::new();
    for case in &cases {
        let table = format!("receive_{}", case.name);
        let binary = |sql: &str, body: &[u8]| {
            fixture::execute_cycle(sql, Some((case.oid, 1, Some(body))), 1, true)
        };
        let simple = |sql| vec![wire::simple_query_frame(sql)];
        let mut excess = case.input_binary.to_vec();
        excess.push(0);
        let empty_update = format!("UPDATE {table} SET v = $1 WHERE k = 2 RETURNING v");
        let update = format!("UPDATE {table} SET v = $1 WHERE k = 1 RETURNING v");
        let select = format!("SELECT v FROM {table} WHERE v = $1 ORDER BY k");
        initial.extend([
            binary(&empty_update, &excess),
            binary(
                &format!("INSERT INTO {table} (k,v) VALUES (1,$1) RETURNING v"),
                case.input_binary,
            ),
            binary(&update, &excess),
            binary(&select, case.input_binary),
        ]);
        restarted.extend([
            binary(&select, case.input_binary),
            simple("BEGIN"),
            simple("SAVEPOINT receive_save"),
            binary(&update, &excess),
            simple("ROLLBACK TO SAVEPOINT receive_save"),
            binary(&update, case.input_binary),
            simple("COMMIT"),
            binary(&select, case.input_binary),
        ]);
    }

    // Act
    let stages = fixture::run_cycles_across_restart(
        "scalar-receive-mutation-restart",
        &matrix::setup_refs(&setup),
        vec![initial, restarted],
    );

    // Assert
    assert_eq!(stages.len(), 2);
    assert_eq!(stages[0].len(), cases.len() * 4);
    assert_eq!(stages[1].len(), cases.len() * 8);
    for ((case, first), second) in cases
        .iter()
        .zip(stages[0].as_chunks::<4>().0)
        .zip(stages[1].as_chunks::<8>().0)
    {
        receive::assert_error(&first[0], "22P03", b'I');
        receive::assert_value(&first[1], case, 1, b'I', b"INSERT 0 1\0");
        receive::assert_error(&first[2], "22P03", b'I');
        receive::assert_value(&first[3], case, 1, b'I', b"SELECT 1\0");
        receive::assert_value(&second[0], case, 1, b'I', b"SELECT 1\0");
        receive::assert_command(&second[1], b"BEGIN\0", b'T');
        receive::assert_command(&second[2], b"SAVEPOINT\0", b'T');
        receive::assert_error(&second[3], "22P03", b'E');
        receive::assert_command(&second[4], b"ROLLBACK\0", b'T');
        receive::assert_value(&second[5], case, 1, b'T', b"UPDATE 1\0");
        receive::assert_command(&second[6], b"COMMIT\0", b'I');
        receive::assert_value(&second[7], case, 1, b'I', b"SELECT 1\0");
    }
}

#[test]
fn should_classify_nested_scalar_receive_errors_with_standard_array_metadata() {
    // Arrange
    let mut payloads: Vec<_> = matrix::SCALAR_CASES
        .iter()
        .filter(|case| case.typlen > 1)
        .map(|case| {
            let mut excess = case.input_binary.to_vec();
            excess.push(0);
            (case.oid, excess, "22P03")
        })
        .collect();
    for (oid, invalid) in receive::invalid_temporal_cases() {
        let mut excess = invalid.clone();
        excess.push(0);
        payloads.extend([(oid, invalid, "22008"), (oid, excess, "22008")]);
    }
    let mut cycles = Vec::new();
    let mut expected = Vec::new();
    for (index, (oid, body, state)) in payloads.into_iter().enumerate() {
        let element = matrix::SCALAR_CASES
            .iter()
            .find(|case| case.oid == oid)
            .expect("scalar fixture");
        let array_type = format!("{}[]", element.sql_type);
        let case = matrix::ARRAY_CASES
            .iter()
            .find(|case| case.sql_type == array_type)
            .expect("standard PostgreSQL array fixture");
        let input = receive::one_element_array(oid, &body);
        for format in [0, 1] {
            let statement = format!("nested_{index}_{format}");
            cycles.push(receive::named_cycle(
                &statement,
                Some(("SELECT $1 AS v", case.oid)),
                Some(&input),
                format,
            ));
            expected.push(Expected {
                case,
                format,
                outcome: Outcome::Error(state),
            });
            cycles.push(receive::named_cycle(
                &statement,
                None,
                Some(case.input_binary),
                format,
            ));
            expected.push(Expected {
                case,
                format,
                outcome: Outcome::Value,
            });
        }
    }

    // Act
    let batches = fixture::run_cycles("scalar-receive-standard-array-errors", &[], cycles);

    // Assert
    assert_eq!(batches.len(), 64);
    receive::assert_batches(&batches, &expected);
}
