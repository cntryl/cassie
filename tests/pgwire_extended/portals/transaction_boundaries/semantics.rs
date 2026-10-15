#[test]
fn should_destroy_preparation_only_portals_at_idle_sync() {
    // Arrange
    let boundary = vec![
        wire::parse_frame("prepared_s", "SELECT 9"),
        wire::bind_frame("prepared_p", "prepared_s", &[]),
        wire::sync_frame(),
    ];
    let tail = vec![vec![wire::execute_frame("prepared_p"), wire::sync_frame()]];

    // Act
    let result = run_script(Prefix::Idle, boundary, "SELECT COUNT(*) FROM wire_tx", tail);

    // Assert
    assert_eq!(
        result
            .boundary
            .iter()
            .map(|frame| frame.0)
            .collect::<Vec<_>>(),
        b"12Z"
    );
    assert_eq!(wire::error_code(&result.tail[0]).as_deref(), Some("26000"));
    assert_eq!(
        result.tail[0].last().map(|frame| frame.1.as_slice()),
        Some(&b"I"[..])
    );
}

#[test]
fn should_reset_unnamed_portals_without_destroying_named_statements() {
    // Arrange
    let boundary = vec![
        wire::parse_frame("named_s", "SELECT 9"),
        wire::bind_frame("", "named_s", &[]),
        wire::sync_frame(),
    ];
    let tail = vec![
        vec![wire::simple_query_frame("SELECT n FROM wire_tx")],
        vec![wire::execute_frame(""), wire::sync_frame()],
        vec![wire::simple_query_frame("ROLLBACK")],
        vec![
            wire::bind_frame("fresh_p", "named_s", &[]),
            wire::execute_frame("fresh_p"),
            wire::sync_frame(),
        ],
    ];

    // Act
    let result = run_script(
        Prefix::Explicit,
        boundary,
        "SELECT COUNT(*) FROM wire_tx",
        tail,
    );

    // Assert
    assert_eq!(
        result.boundary.last().map(|frame| frame.1.as_slice()),
        Some(&b"T"[..])
    );
    assert_eq!(
        wire::data_rows(&result.tail[0]),
        vec![vec![Some("1".to_string())]]
    );
    assert_eq!(wire::error_code(&result.tail[1]).as_deref(), Some("26000"));
    assert_eq!(wire::error_code(&result.tail[3]), None);
    assert_eq!(
        wire::data_rows(&result.tail[3]),
        vec![vec![Some("9".to_string())]]
    );
    assert_eq!(
        wire::data_rows(&result.terminal),
        vec![vec![Some("0".to_string())]]
    );
}

#[test]
fn should_close_named_portals_derived_from_an_unnamed_statement_on_simple_query() {
    // Arrange
    let boundary = vec![
        wire::parse_frame("", "SELECT 9"),
        wire::bind_frame("named_derived", "", &[]),
        wire::sync_frame(),
    ];
    let tail = vec![
        vec![wire::simple_query_frame("")],
        vec![wire::execute_frame("named_derived"), wire::sync_frame()],
    ];

    // Act
    let result = run_script(
        Prefix::Explicit,
        boundary,
        "SELECT COUNT(*) FROM wire_tx",
        tail,
    );

    // Assert
    assert_eq!(
        result.tail[0]
            .iter()
            .map(|frame| frame.0)
            .collect::<Vec<_>>(),
        b"IZ"
    );
    assert_eq!(
        result.tail[0].last().map(|frame| frame.1.as_slice()),
        Some(&b"T"[..])
    );
    assert_eq!(wire::error_code(&result.tail[1]).as_deref(), Some("26000"));
    assert_eq!(
        wire::data_rows(&result.terminal),
        vec![vec![Some("0".to_string())]]
    );
}
