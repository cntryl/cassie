use super::*;

#[test]
fn should_return_numeric_log_as_binary_numeric_for_a_binary_parameter() {
    // Arrange
    use crate::support_type_metadata_contract as fixture;

    let statement = "numeric_log_binary";
    let portal = "numeric_log_binary_portal";
    let sql = "SELECT LOG($1) AS numeric_log";
    let numeric_100 = [0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x64];
    let numeric_log_2 = [0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x02];
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(statement, sql, &[1700])),
        Step::ThroughReady(vec![
            wire::describe_statement_frame(statement),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(portal, statement, &[1], &[Some(&numeric_100[..])], &[1]),
            wire::describe_portal_frame(portal),
            wire::execute_frame(portal),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("postgresql18-numeric-log-binary", steps);

    // Assert
    assert_eq!(
        batches[0].iter().map(|(tag, _)| *tag).collect::<Vec<_>>(),
        vec![b'1']
    );
    assert_eq!(wire::error_code(&batches[1]), None);
    let parameter_description = batches[1]
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("numeric LOG ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameter_description.1),
        vec![1700]
    );
    let statement_description = batches[1]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("numeric LOG statement RowDescription");
    assert_eq!(
        wire::parse_row_description(&statement_description.1)
            .iter()
            .map(|column| (column.type_oid, column.type_mod))
            .collect::<Vec<_>>(),
        vec![(1700, -1)]
    );

    let portal_frames = &batches[2];
    assert_eq!(wire::error_code(portal_frames), None);
    let portal_description = portal_frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("binary numeric LOG portal RowDescription");
    let column = &wire::parse_row_description(&portal_description.1)[0];
    assert_eq!(
        (column.type_oid, column.type_mod, column.format_code),
        (1700, -1, 1)
    );
    assert_eq!(
        fixture::data_row_payloads(portal_frames),
        vec![fixture::one_field_row(&numeric_log_2)]
    );
}
