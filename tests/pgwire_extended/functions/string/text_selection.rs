//! Typed text selection, strict NULLs and recovery on actual pgwire phases.

use super::{run_script, wire, Step};

#[test]
fn should_infer_text_selection_parameters_given_unknown_oids_when_binary_results_are_described_and_executed(
) {
    // Arrange
    let cases = [
        (
            "SELECT split_part($1,$2,$3) AS selected",
            vec!["Aé🙂::B::", "::", "2"],
            vec![25, 25, 23],
            25,
            b"B".to_vec(),
        ),
        (
            "SELECT substr($1,$2,$3) AS selected",
            vec!["Aé🙂Z", "2", "2"],
            vec![25, 23, 23],
            25,
            "é🙂".as_bytes().to_vec(),
        ),
        (
            "SELECT strpos($1,$2) AS selected",
            vec!["Aé🙂Z", "🙂"],
            vec![25, 25],
            23,
            3_i32.to_be_bytes().to_vec(),
        ),
        (
            "SELECT TRIM(BOTH $1 FROM $2) AS selected",
            vec!["é🙂", "é🙂x🙂é"],
            vec![25, 25],
            25,
            b"x".to_vec(),
        ),
    ];

    // Act
    for (ordinal, (sql, parameters, input_oids, output_oid, expected)) in
        cases.into_iter().enumerate()
    {
        let parameter_bytes = parameters
            .iter()
            .map(|value| Some(value.as_bytes()))
            .collect::<Vec<_>>();
        let batches = run_script(
            &format!("text-selection-binary-{ordinal}"),
            vec![
                Step::ParseFlush(wire::parse_frame_with_types(
                    "selected",
                    sql,
                    &vec![0; parameters.len()],
                )),
                Step::ThroughReady(vec![
                    wire::describe_statement_frame("selected"),
                    wire::sync_frame(),
                ]),
                Step::ThroughReady(vec![
                    wire::bind_frame_with_formats(
                        "output",
                        "selected",
                        &[],
                        &parameter_bytes,
                        &[1],
                    ),
                    wire::describe_portal_frame("output"),
                    wire::execute_frame("output"),
                    wire::sync_frame(),
                ]),
            ],
        );

        // Assert
        for batch in &batches {
            assert_eq!(wire::error_code(batch), None, "{sql}");
        }
        let parameters = batches[1]
            .iter()
            .find(|frame| frame.0 == b't')
            .expect("ParameterDescription");
        assert_eq!(
            wire::parse_parameter_description(&parameters.1),
            input_oids,
            "{sql}"
        );
        for batch in &batches[1..] {
            let description = batch
                .iter()
                .find(|frame| frame.0 == b'T')
                .expect("RowDescription");
            assert_eq!(
                wire::parse_row_description(&description.1)[0].type_oid,
                output_oid,
                "{sql}"
            );
        }
        let row = batches[2]
            .iter()
            .find(|frame| frame.0 == b'D')
            .expect("binary DataRow");
        assert_eq!(&row.1[..2], &1_i16.to_be_bytes(), "{sql}");
        assert_eq!(
            &row.1[2..6],
            &i32::try_from(expected.len())
                .expect("bounded field length")
                .to_be_bytes(),
            "{sql}"
        );
        assert_eq!(&row.1[6..], expected.as_slice(), "{sql}");
    }
}

#[test]
fn should_preserve_text_error_states_given_strict_null_controls_when_the_connection_is_reused() {
    // Arrange
    let batches = run_script(
        "text-selection-errors-and-recovery",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT substring(NULL::TEXT,1,-1),split_part(NULL::TEXT,',',0)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT substring('abc',1,-1)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT split_part('a,b',',',0)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT split_part('a,b',',','2147483648')",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame("SELECT substr('abc','bad')")]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT substring('Aé🙂Z',2,2),strpos('Aé🙂Z','🙂')",
            )]),
        ],
    );

    // Act
    let null_control = &batches[0];
    let negative_length = &batches[1];
    let zero_field = &batches[2];
    let overflow = &batches[3];
    let invalid_integer = &batches[4];
    let recovery = &batches[5];

    // Assert
    assert_eq!(wire::error_code(null_control), None);
    assert_eq!(wire::data_rows(null_control), vec![vec![None, None]]);
    for (batch, code) in [
        (negative_length, "22011"),
        (zero_field, "22023"),
        (overflow, "22003"),
        (invalid_integer, "22P02"),
    ] {
        assert_eq!(wire::error_code(batch).as_deref(), Some(code));
        assert!(wire::data_rows(batch).is_empty());
        assert!(!batch.iter().any(|frame| frame.0 == b'C'));
        assert_eq!(batch.last().map(|frame| frame.0), Some(b'Z'));
    }
    assert_eq!(wire::error_code(recovery), None);
    assert_eq!(
        wire::data_rows(recovery),
        vec![vec![Some("é🙂".into()), Some("3".into())]]
    );
}

#[test]
fn should_reject_invalid_text_overloads_given_typed_nulls_when_simple_and_parse_phases_are_checked()
{
    // Arrange
    let cases = [
        "SELECT strpos(NULL::INT,'a')",
        "SELECT split_part('a,b',',',NULL::TEXT)",
        "SELECT substring('abc',1.5)",
    ];
    let mut steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT substr('abc','2','1'),substring('abc',2,'1'),substring('abc','2',1)",
    )])];
    for sql in cases {
        steps.push(Step::ThroughReady(vec![wire::simple_query_frame(sql)]));
        steps.push(Step::ParseFlush(wire::parse_frame_with_types("", sql, &[])));
        steps.push(Step::ThroughReady(vec![wire::sync_frame()]));
    }

    // Act
    let batches = run_script("text-selection-invalid-overloads", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("b".into()), Some("b".into()), Some("b".into())]]
    );
    for (sql, phases) in cases.iter().zip(batches[1..].chunks_exact(3)) {
        for batch in &phases[..2] {
            assert_eq!(wire::error_code(batch).as_deref(), Some("42883"), "{sql}");
            assert!(wire::data_rows(batch).is_empty());
            assert!(!batch.iter().any(|frame| frame.0 == b'1' || frame.0 == b'C'));
        }
        assert_eq!(phases[2].last().map(|frame| frame.0), Some(b'Z'));
    }
}

#[test]
fn should_preserve_regex_overload_identity_given_unknown_substring_parameters_when_parse_describes_pending_execution(
) {
    // Arrange
    let batches = run_script(
        "text-selection-regex-overload-boundary",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                "regexp",
                "SELECT substring($1 FROM $2 FOR $3)",
                &[0, 0, 0],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("regexp"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame("output", "regexp", &["abc", "2", "1"]),
                wire::execute_frame("output"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let description = &batches[1];
    let execution = &batches[2];

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(wire::error_code(description), None);
    let parameters = description
        .iter()
        .find(|frame| frame.0 == b't')
        .expect("ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameters.1),
        vec![25, 25, 25]
    );
    // This is an explicit staged rejection, not a PostgreSQL regex execution pass.
    assert_eq!(wire::error_code(execution).as_deref(), Some("0A000"));
    assert!(wire::data_rows(execution).is_empty());
}
