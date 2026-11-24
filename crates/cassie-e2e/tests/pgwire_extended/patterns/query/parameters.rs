#[test]
fn should_bind_like_escape_parameters_over_pgwire() {
    // Arrange
    let sql = "SELECT $1 NOT LIKE $2 ESCAPE $3 AS escaped_not_like";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            "pattern_parameters",
            sql,
            &[25, 25, 25],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame("pattern_parameters"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame(
                "pattern_portal",
                "pattern_parameters",
                &["ab", "a!_%", "!"],
            ),
            wire::describe_portal_frame("pattern_portal"),
            wire::execute_frame("pattern_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("issue-799-bound-similar-like-escape-wire", steps);
    let described = &batches[1];
    let executed = &batches[2];

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(wire::error_code(described), None);
    assert_eq!(
        wire::parse_parameter_description(
            &described
                .iter()
                .find(|frame| frame.0 == b't')
                .expect("pattern ParameterDescription")
                .1
        ),
        vec![25; 3]
    );
    let statement_description = described
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("pattern statement RowDescription");
    assert_eq!(
        wire::parse_row_description(&statement_description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![16]
    );
    assert_eq!(wire::error_code(executed), None);
    assert_eq!(
        wire::data_rows(executed),
        vec![vec![Some("t".to_owned())]]
    );
}
