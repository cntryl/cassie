use super::{run_script, wire, Step};

#[test]
fn should_preserve_mod_result_identity_for_each_supported_numeric_width() {
    // Arrange
    let statement = "mod_result_identity";
    let portal = "mod_result_identity_portal";
    let sql = "SELECT MOD($1, $2), MOD($3, $4), MOD($5, $6), MOD($7, $8)";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            statement,
            sql,
            &[21, 21, 23, 23, 20, 20, 1700, 1700],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame(statement),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame(
                portal,
                statement,
                &["5", "3", "5", "3", "5", "3", "5", "3"],
            ),
            wire::describe_portal_frame(portal),
            wire::execute_frame(portal),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("postgresql18-mod-result-identities", steps);

    // Assert
    assert_eq!(batches[0].first().map(|(tag, _)| *tag), Some(b'1'));
    let described = &batches[1];
    assert_eq!(wire::error_code(described), None);
    let parameter_types = described
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("MOD ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameter_types.1),
        vec![21, 21, 23, 23, 20, 20, 1700, 1700]
    );
    let description = described
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("MOD statement RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.type_mod))
            .collect::<Vec<_>>(),
        vec![(21, 2, -1), (23, 4, -1), (20, 8, -1), (1700, -1, -1)]
    );
    assert_eq!(wire::error_code(&batches[2]), None);
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![
            Some("2".to_owned()),
            Some("2".to_owned()),
            Some("2".to_owned()),
            Some("2".to_owned()),
        ]]
    );
}
