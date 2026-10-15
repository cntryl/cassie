type WireFrames = [(u8, Vec<u8>)];
type NamedWireStage<'a> = (&'a str, &'a WireFrames);

fn assert_error_free_stages(context: &str, stages: &[NamedWireStage<'_>]) {
    for (stage, frames) in stages {
        assert_eq!(
            wire::error_code(frames),
            None,
            "{context} {stage} ErrorResponse: {:?}",
            frames
                .iter()
                .filter(|frame| frame.0 == b'E')
                .map(|frame| wire::parse_error_fields(&frame.1))
                .collect::<Vec<_>>()
        );
    }
}

fn assert_binary_row_constructor_control(
    parsed: &[(u8, Vec<u8>)],
    described: &[(u8, Vec<u8>)],
    executed: &[(u8, Vec<u8>)],
) {
    assert_error_free_stages(
        "row-constructor control",
        &[
            ("Parse", parsed),
            ("Describe", described),
            ("Execute", executed),
        ],
    );
    let parameters = described
        .iter()
        .find(|frame| frame.0 == b't')
        .expect("row-constructor control ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameters.1),
        vec![23, 23]
    );
    let description = described
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("row-constructor control RowDescription");
    assert_eq!(wire::parse_row_description(&description.1)[0].type_oid, 16);
    let data_row = executed
        .iter()
        .find(|frame| frame.0 == b'D')
        .expect("binary row-constructor control DataRow");
    assert_eq!(&data_row.1[6..], &[1]);
}

fn assert_binary_row_subquery_result(
    parsed: &[(u8, Vec<u8>)],
    described: &[(u8, Vec<u8>)],
    executed: &[(u8, Vec<u8>)],
) {
    assert_error_free_stages(
        "row-subquery",
        &[
            ("Parse", parsed),
            ("Describe", described),
            ("Execute", executed),
        ],
    );
    let parameters = described
        .iter()
        .find(|frame| frame.0 == b't')
        .expect("row-subquery ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameters.1),
        vec![23, 23]
    );
    let statement_description = described
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("row-subquery statement RowDescription");
    let statement_column = &wire::parse_row_description(&statement_description.1)[0];
    assert_eq!(statement_column.name, "matches");
    assert_eq!(statement_column.type_oid, 16);

    let portal_description = executed
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("binary row-subquery portal RowDescription");
    let portal_column = &wire::parse_row_description(&portal_description.1)[0];
    assert_eq!(portal_column.type_oid, 16);
    assert_eq!(portal_column.format_code, 1);
    let data_row = executed
        .iter()
        .find(|frame| frame.0 == b'D')
        .expect("binary row-subquery DataRow");
    assert_eq!(
        i16::from_be_bytes(data_row.1[0..2].try_into().expect("field count")),
        1
    );
    assert_eq!(
        i32::from_be_bytes(data_row.1[2..6].try_into().expect("field length")),
        1
    );
    assert_eq!(data_row.1[6], 1);
}

#[test]
fn should_execute_binary_row_subquery_with_described_metadata() {
    // Arrange
    let control_statement = "row_constructor_control";
    let target_statement = "row_subquery";
    let control_portal = "row_constructor_control_binary";
    let target_portal = "row_subquery_binary";
    let control_sql = "SELECT ROW($1::INT4, $2::INT4) = \
                       ROW(1::INT4, 10::INT4) AS matches";
    let sql = "SELECT ROW($1::INT4, $2::INT4) = (\
                   SELECT candidate.x, candidate.y \
                   FROM issue_762_binary_row_values AS candidate \
               ) AS matches";
    let first_value = 1_i32.to_be_bytes();
    let second_value = 10_i32.to_be_bytes();
    let batches = run_script(
        "issue-762-row-subquery-binary-parameters",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_762_binary_row_values (x INT4, y INT4)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO issue_762_binary_row_values VALUES (1, 10)",
            )]),
            Step::ParseFlush(wire::parse_frame_with_types(
                control_statement,
                control_sql,
                &[0, 0],
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
                    &[Some(&first_value), Some(&second_value)],
                    &[1],
                ),
                wire::describe_portal_frame(control_portal),
                wire::execute_frame(control_portal),
                wire::sync_frame(),
            ]),
            Step::ParseFlush(wire::parse_frame_with_types(target_statement, sql, &[0, 0])),
            Step::ThroughReady(vec![
                wire::describe_statement_frame(target_statement),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame_with_formats(
                    target_portal,
                    target_statement,
                    &[1],
                    &[Some(&first_value), Some(&second_value)],
                    &[1],
                ),
                wire::describe_portal_frame(target_portal),
                wire::execute_frame(target_portal),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let control_parsed = &batches[2];
    let control_described = &batches[3];
    let control_executed = &batches[4];
    let parsed = &batches[5];
    let described = &batches[6];
    let executed = &batches[7];

    // Assert
    assert_error_free_stages(
        "row fixture setup",
        &[("Setup", &batches[0]), ("Insert", &batches[1])],
    );
    assert_binary_row_constructor_control(control_parsed, control_described, control_executed);
    assert_binary_row_subquery_result(parsed, described, executed);
}
