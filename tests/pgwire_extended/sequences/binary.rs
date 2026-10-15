fn assert_direct_bigint_control(
    parsed: &[(u8, Vec<u8>)],
    described: &[(u8, Vec<u8>)],
    executed: &[(u8, Vec<u8>)],
    expected_value: &[u8],
) {
    assert_eq!(wire::error_code(parsed), None);
    assert_eq!(wire::error_code(described), None);
    assert_eq!(wire::error_code(executed), None);
    assert_eq!(
        wire::parse_parameter_description(
            &described
                .iter()
                .find(|frame| frame.0 == b't')
                .expect("direct BIGINT ParameterDescription")
                .1
        ),
        vec![20]
    );
    let portal_description = executed
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("direct BIGINT binary portal RowDescription");
    let portal_column = &wire::parse_row_description(&portal_description.1)[0];
    assert_eq!((portal_column.type_oid, portal_column.format_code), (20, 1));
    let data_row = executed
        .iter()
        .find(|frame| frame.0 == b'D')
        .expect("direct BIGINT binary DataRow");
    assert_eq!(&data_row.1[6..], expected_value);
}

fn assert_scalar_bigint_binary_result(
    parsed: &[(u8, Vec<u8>)],
    described: &[(u8, Vec<u8>)],
    executed: &[(u8, Vec<u8>)],
    expected_value: &[u8],
) {
    for (stage, frames) in [
        ("Parse", parsed),
        ("Describe", described),
        ("binary Bind/Execute", executed),
    ] {
        let errors = frames
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>();
        assert_eq!(
            wire::error_code(frames),
            None,
            "scalar BIGINT {stage} ErrorResponse: {errors:?}"
        );
    }
    assert_eq!(
        wire::parse_parameter_description(
            &described
                .iter()
                .find(|frame| frame.0 == b't')
                .expect("scalar BIGINT ParameterDescription")
                .1
        ),
        vec![20]
    );
    assert_eq!(
        wire::parse_row_description(
            &described
                .iter()
                .find(|frame| frame.0 == b'T')
                .expect("scalar BIGINT statement RowDescription")
                .1
        )[0]
        .type_oid,
        20
    );
    let portal_description = executed
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("binary scalar BIGINT portal RowDescription");
    let portal_column = &wire::parse_row_description(&portal_description.1)[0];
    assert_eq!((portal_column.type_oid, portal_column.format_code), (20, 1));
    let data_row = executed
        .iter()
        .find(|frame| frame.0 == b'D')
        .expect("binary scalar BIGINT DataRow");
    assert_eq!(&data_row.1[..2], &1_i16.to_be_bytes());
    assert_eq!(&data_row.1[2..6], &8_i32.to_be_bytes());
    assert_eq!(&data_row.1[6..], expected_value);
}

#[test]
fn should_preserve_bigint_parameter_identity_through_binary_scalar_execution() {
    // Arrange
    let control_statement = "bigint_parameter_control";
    let target_statement = "scalar_subquery_bigint";
    let control_portal = "bigint_parameter_control_binary";
    let target_portal = "scalar_subquery_bigint_binary";
    let high_value = 9_007_199_254_740_993_i64.to_be_bytes();
    let batches = run_script(
        "issue-762-scalar-subquery-bigint-binary-wire",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                control_statement,
                "SELECT $1::BIGINT AS scalar_value",
                &[0],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame(control_statement),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame_with_formats(
                    control_portal,
                    control_statement,
                    &[1],
                    &[Some(&high_value)],
                    &[1],
                ),
                wire::describe_portal_frame(control_portal),
                wire::execute_frame(control_portal),
                wire::sync_frame(),
            ]),
            Step::ParseFlush(wire::parse_frame_with_types(
                target_statement,
                "SELECT (SELECT $1::BIGINT) AS scalar_value",
                &[0],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame(target_statement),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame_with_formats(
                    target_portal,
                    target_statement,
                    &[1],
                    &[Some(&high_value)],
                    &[1],
                ),
                wire::describe_portal_frame(target_portal),
                wire::execute_frame(target_portal),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let control_parsed = &batches[0];
    let control_described = &batches[1];
    let control_executed = &batches[2];
    let parsed = &batches[3];
    let described = &batches[4];
    let executed = &batches[5];

    // Assert
    assert_direct_bigint_control(
        control_parsed,
        control_described,
        control_executed,
        &high_value,
    );
    assert_scalar_bigint_binary_result(parsed, described, executed, &high_value);
}
