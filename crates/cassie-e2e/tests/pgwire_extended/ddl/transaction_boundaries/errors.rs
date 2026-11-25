#[test]
fn should_reject_mixed_simple_ddl_before_executing_the_prefix() {
    // Arrange
    let boundary = vec![wire::simple_query_frame(
        "INSERT INTO wire_tx (n) VALUES (1); CREATE TABLE mixed_ddl (n INT)",
    )];

    // Act
    let (_, _, boundary, after) = run_cycle(boundary, "SELECT COUNT(*) FROM wire_tx", false);

    // Assert
    assert_eq!(
        boundary.iter().map(|frame| frame.0).collect::<Vec<_>>(),
        b"EZ"
    );
    assert_eq!(wire::error_code(&boundary).as_deref(), Some("0A000"));
    assert_eq!(wire::data_rows(&after), vec![vec![Some("0".to_string())]]);
}

#[test]
fn should_reject_dml_after_completed_extended_ddl_until_sync() {
    // Arrange
    let boundary = vec![
        wire::parse_frame("ddl_s", "CREATE TABLE standalone_ddl (n INT)"),
        wire::bind_frame("ddl_p", "ddl_s", &[]),
        wire::execute_frame("ddl_p"),
        wire::parse_frame("later_s", "INSERT INTO wire_tx (n) VALUES (2)"),
        wire::bind_frame("later_p", "later_s", &[]),
        wire::execute_frame("later_p"),
        wire::sync_frame(),
    ];
    let observer = "SELECT COUNT(*) FROM wire_tx; SELECT COUNT(*) FROM information_schema.tables WHERE table_name = 'standalone_ddl'";

    // Act
    let result = run_script(Prefix::Idle, boundary, observer, Vec::new());

    // Assert
    assert_eq!(
        result
            .boundary
            .iter()
            .map(|frame| frame.0)
            .collect::<Vec<_>>(),
        b"12C12EZ"
    );
    assert_eq!(wire::error_code(&result.boundary).as_deref(), Some("0A000"));
    assert_eq!(
        result.boundary.last().map(|frame| frame.1.as_slice()),
        Some(&b"I"[..])
    );
    assert_eq!(
        wire::data_rows(&result.after),
        vec![vec![Some("0".to_string())], vec![Some("1".to_string())]]
    );
}

#[test]
fn should_reject_a_second_extended_ddl_until_sync() {
    // Arrange
    let boundary = vec![
        wire::parse_frame("first_s", "CREATE TABLE first_ddl (n INT)"),
        wire::bind_frame("first_p", "first_s", &[]),
        wire::execute_frame("first_p"),
        wire::parse_frame("later_s", "CREATE TABLE later_ddl (n INT)"),
        wire::bind_frame("later_p", "later_s", &[]),
        wire::execute_frame("later_p"),
        wire::sync_frame(),
    ];
    let observer = "SELECT COUNT(*) FROM information_schema.tables WHERE table_name = 'first_ddl'; SELECT COUNT(*) FROM information_schema.tables WHERE table_name = 'later_ddl'";

    // Act
    let result = run_script(Prefix::Idle, boundary, observer, Vec::new());

    // Assert
    assert_eq!(
        result
            .boundary
            .iter()
            .map(|frame| frame.0)
            .collect::<Vec<_>>(),
        b"12C12EZ"
    );
    assert_eq!(wire::error_code(&result.boundary).as_deref(), Some("0A000"));
    assert_eq!(
        wire::data_rows(&result.after),
        vec![vec![Some("1".to_string())], vec![Some("0".to_string())]]
    );
}
