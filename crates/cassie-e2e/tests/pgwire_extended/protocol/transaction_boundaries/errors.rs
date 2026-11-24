#[test]
fn should_roll_back_an_idle_simple_query_batch_on_error() {
    // Arrange
    let boundary = vec![wire::simple_query_frame(
        "INSERT INTO wire_tx (n) VALUES (1); SELECT n FROM wire_tx WHERE (n / 0) = 1; INSERT INTO wire_tx (n) VALUES (2)",
    )];

    // Act
    let (_, _, boundary, after) = run_cycle(boundary, "SELECT COUNT(*) FROM wire_tx", false);

    // Assert
    assert_eq!(
        boundary.iter().map(|frame| frame.0).collect::<Vec<_>>(),
        b"CEZ"
    );
    assert_eq!(wire::error_code(&boundary).as_deref(), Some("22012"));
    assert_eq!(
        boundary.last().map(|frame| frame.1.as_slice()),
        Some(&b"I"[..])
    );
    assert_eq!(wire::data_rows(&after), vec![vec![Some("0".to_string())]]);
}

#[test]
fn should_discard_pending_implicit_work_on_simple_query_split_error() {
    // Arrange
    let boundary = vec![wire::simple_query_frame("SELECT 'unterminated")];

    // Act
    let result = run_script(
        Prefix::Implicit,
        boundary,
        "SELECT COUNT(*) FROM wire_tx",
        Vec::new(),
    );

    // Assert
    assert_eq!(wire::error_code(&result.boundary).as_deref(), Some("42601"));
    assert_eq!(
        result.boundary.last().map(|frame| frame.1.as_slice()),
        Some(&b"I"[..])
    );
    assert_eq!(
        wire::data_rows(&result.after),
        vec![vec![Some("0".to_string())]]
    );
}
