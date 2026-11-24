#[test]
fn should_preserve_negative_scale_typmod_rounding_at_overflow() {
    // Arrange
    let sql = "SELECT CAST($1 AS NUMERIC(3, -2)) AS rounded_value";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            "negative_numeric_scale",
            sql,
            &[1700],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame("negative_numeric_scale"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame(
                "negative_numeric_positive_portal",
                "negative_numeric_scale",
                &["12345"],
            ),
            wire::describe_portal_frame("negative_numeric_positive_portal"),
            wire::execute_frame("negative_numeric_positive_portal"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame(
                "negative_numeric_negative_portal",
                "negative_numeric_scale",
                &["-12345"],
            ),
            wire::describe_portal_frame("negative_numeric_negative_portal"),
            wire::execute_frame("negative_numeric_negative_portal"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT CAST('99950' AS NUMERIC(3, -2))",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 42")]),
    ];

    // Act
    let batches = run_script("postgresql18-negative-numeric-scale", steps);

    // Assert
    assert_eq!(
        batches[0].first().map(|(tag, _)| *tag),
        Some(b'1'),
        "Parse should accept NUMERIC(3, -2); SQLSTATE: {:?}",
        wire::error_code(&batches[0])
    );
    assert_eq!(wire::error_code(&batches[1]), None);
    assert_eq!(
        wire::parse_parameter_description(
            &batches[1]
                .iter()
                .find(|(tag, _)| *tag == b't')
                .expect("negative-scale ParameterDescription")
                .1
        ),
        vec![1700]
    );
    let statement_description = batches[1]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("negative-scale statement RowDescription");
    let statement_column = &wire::parse_row_description(&statement_description.1)[0];
    assert_eq!(statement_column.type_oid, 1700);
    assert_eq!(statement_column.type_mod, 198_658);

    for (batch, expected) in [(&batches[2], "12300"), (&batches[3], "-12300")] {
        assert_eq!(wire::error_code(batch), None);
        let description = batch
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("negative-scale portal RowDescription");
        let column = &wire::parse_row_description(&description.1)[0];
        assert_eq!((column.type_oid, column.type_mod), (1700, 198_658));
        assert_eq!(
            wire::data_rows(batch),
            vec![vec![Some(expected.to_owned())]]
        );
    }
    assert_eq!(wire::error_code(&batches[4]).as_deref(), Some("22003"));
    assert_eq!(
        wire::data_rows(&batches[4]),
        Vec::<Vec<Option<String>>>::new()
    );
    assert_eq!(wire::error_code(&batches[5]), None);
    assert_eq!(
        wire::data_rows(&batches[5]),
        vec![vec![Some("42".to_owned())]]
    );
}
