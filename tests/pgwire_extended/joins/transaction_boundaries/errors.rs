#[test]
fn should_roll_back_a_joined_query_on_error() {
    // Arrange
    let boundary = vec![wire::simple_query_frame(
        "INSERT INTO wire_tx (n) VALUES (2); SELECT n FROM wire_tx WHERE (n / 0) = 1; INSERT INTO wire_tx (n) VALUES (3)",
    )];

    // Act
    let (_, before, boundary, after) = run_boundary(boundary, "SELECT COUNT(*) FROM wire_tx");

    // Assert
    assert_eq!(wire::data_rows(&before), vec![vec![Some("0".to_string())]]);
    assert_eq!(
        boundary.iter().map(|frame| frame.0).collect::<Vec<_>>(),
        b"CEZ"
    );
    assert_eq!(wire::error_code(&boundary).as_deref(), Some("22012"));
    assert_eq!(
        boundary.last().map(|frame| frame.1.as_slice()),
        Some(&b"I"[..])
    );
    assert_eq!(wire::error_code(&after), None);
    assert_eq!(wire::data_rows(&after), vec![vec![Some("0".to_string())]]);
}
