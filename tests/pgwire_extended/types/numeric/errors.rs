#[test]
fn should_report_numeric_bind_input_errors_with_postgresql_sqlstates() {
    // Arrange
    let statement_cases = [
        (
            "numeric_bad_text_input",
            "numeric_bad_text_portal",
            0,
            &b"not-a-number"[..],
            "22P02",
        ),
        (
            "numeric_bad_binary_digit",
            "numeric_bad_binary_digit_portal",
            1,
            &[0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x27, 0x10][..],
            "22P03",
        ),
        (
            "numeric_short_binary_header",
            "numeric_short_binary_header_portal",
            1,
            &[0x00, 0x01][..],
            "08P01",
        ),
    ];
    let mut steps = Vec::new();
    for (statement, portal, parameter_format, value, _) in statement_cases {
        steps.push(Step::ParseFlush(wire::parse_frame_with_types(
            statement,
            "SELECT $1::NUMERIC AS amount",
            &[1700],
        )));
        steps.push(Step::ThroughReady(vec![
            wire::describe_statement_frame(statement),
            wire::sync_frame(),
        ]));
        steps.push(Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                portal,
                statement,
                &[parameter_format],
                &[Some(value)],
                &[0],
            ),
            wire::describe_portal_frame(portal),
            wire::execute_frame(portal),
            wire::sync_frame(),
        ]));
        steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT 42 AS recovered",
        )]));
    }

    // Act
    let batches = run_script("postgresql18-numeric-input-error-sqlstates", steps);

    // Assert
    assert_eq!(batches.len(), statement_cases.len() * 4);
    for (index, (statement, _, _, _, expected_state)) in statement_cases.iter().enumerate() {
        let offset = index * 4;
        assert_eq!(
            wire::error_code(&batches[offset]),
            None,
            "{statement} Parse"
        );
        let parameter_description = batches[offset + 1]
            .iter()
            .find(|(tag, _)| *tag == b't')
            .expect("numeric ParameterDescription");
        assert_eq!(
            wire::parse_parameter_description(&parameter_description.1),
            vec![1700],
            "{statement} parameter OID"
        );
        let error_batch = &batches[offset + 2];
        assert_eq!(
            wire::error_code(error_batch).as_deref(),
            Some(*expected_state),
            "{statement} wire error fields: {:?}",
            error_batch
                .iter()
                .filter(|(tag, _)| *tag == b'E')
                .map(|(_, payload)| wire::parse_error_fields(payload))
                .collect::<Vec<_>>()
        );
        assert!(!error_batch
            .iter()
            .any(|(tag, _)| matches!(*tag, b'T' | b'D' | b'C')));
        assert_eq!(
            error_batch.last().map(|frame| frame.1.as_slice()),
            Some(&b"I"[..]),
            "{statement} should return ReadyForQuery after Bind error"
        );
        assert_eq!(wire::error_code(&batches[offset + 3]), None);
        assert_eq!(
            wire::data_rows(&batches[offset + 3]),
            vec![vec![Some("42".to_owned())]],
            "{statement} session recovery"
        );
    }
}
