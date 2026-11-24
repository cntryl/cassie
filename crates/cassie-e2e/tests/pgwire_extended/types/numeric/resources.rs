#[test]
fn should_reject_numeric_exponents_beyond_postgresql_integer_digit_limit() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT CAST('1e131072' AS NUMERIC)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 42")]),
    ];

    // Act
    let batches = run_script("postgresql18-numeric-absolute-range", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("22003"));
    assert_eq!(
        wire::data_rows(&batches[0]),
        Vec::<Vec<Option<String>>>::new()
    );
    assert_eq!(wire::error_code(&batches[1]), None);
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("42".to_owned())]]
    );
}

#[test]
fn should_enforce_postgresql_numeric_precision_scale_limits() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT CAST(NULL AS NUMERIC(1000, 0))",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT CAST(NULL AS NUMERIC(1000, 1000))",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT CAST('1e-16383' AS NUMERIC) > 0",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT CAST('1e131071' AS NUMERIC) > 0",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT CAST('1e-16384' AS NUMERIC)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT CAST('1e131072' AS NUMERIC)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT CAST(NULL AS NUMERIC(1001, 0))",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT CAST(NULL AS NUMERIC(1000, 1001))",
        )]),
    ];

    // Act
    let batches = run_script("postgresql18-numeric-precision-scale-limits", steps);

    // Assert
    for (index, batch) in batches.iter().take(4).enumerate() {
        assert_eq!(wire::error_code(batch), None, "valid boundary {index}");
    }
    for (index, expected_state) in [(4, "22003"), (5, "22003"), (6, "22023"), (7, "22023")] {
        assert_eq!(
            wire::error_code(&batches[index]).as_deref(),
            Some(expected_state),
            "rejection boundary {index}"
        );
    }
    for (index, expected_typmod) in [(0, 65_536_004), (1, 65_537_004)] {
        let description = batches[index]
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("numeric boundary RowDescription");
        let column = &wire::parse_row_description(&description.1)[0];
        assert_eq!(column.type_oid, 1700);
        assert_eq!(column.type_mod, expected_typmod);
        assert_eq!(wire::data_rows(&batches[index]), vec![vec![None]]);
    }
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some("t".to_owned())]]
    );
    assert_eq!(
        wire::data_rows(&batches[3]),
        vec![vec![Some("t".to_owned())]]
    );
}
