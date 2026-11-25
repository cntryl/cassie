#[test]
fn should_fail_explicit_transactions_after_simple_query_output_limit_errors() {
    // Arrange
    let projections = (0..17)
        .map(|index| format!("payload AS p{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let boundary = vec![wire::simple_query_frame(&format!(
        "SELECT {projections} FROM wire_tx WHERE n = 0"
    ))];
    let tail = vec![vec![wire::simple_query_frame("ROLLBACK")]];

    // Act
    let result = run_script(
        Prefix::ExplicitOutputError,
        boundary,
        "SELECT COUNT(*) FROM wire_tx WHERE n = 1",
        tail,
    );

    // Assert
    assert_eq!(wire::error_code(&result.boundary).as_deref(), Some("54000"));
    assert_eq!(
        result
            .boundary
            .iter()
            .map(|frame| frame.0)
            .collect::<Vec<_>>(),
        b"TEZ"
    );
    assert_eq!(
        result.boundary.last().map(|frame| frame.1.as_slice()),
        Some(&b"E"[..])
    );
    assert_eq!(
        result.tail[0].last().map(|frame| frame.1.as_slice()),
        Some(&b"I"[..])
    );
    assert_eq!(
        wire::data_rows(&result.terminal),
        vec![vec![Some("0".to_string())]]
    );
}
