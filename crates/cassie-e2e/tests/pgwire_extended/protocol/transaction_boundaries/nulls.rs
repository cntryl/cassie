#[test]
fn should_destroy_the_unnamed_statement_on_empty_simple_query() {
    // Arrange
    let boundary = vec![
        wire::parse_frame("", "SELECT 9"),
        wire::bind_frame("named_from_unnamed", "", &[]),
        wire::sync_frame(),
    ];
    let tail = vec![
        vec![wire::simple_query_frame("")],
        vec![
            wire::bind_frame("fresh_p", "", &[]),
            wire::execute_frame("fresh_p"),
            wire::sync_frame(),
        ],
    ];

    // Act
    let result = run_script(Prefix::Idle, boundary, "SELECT COUNT(*) FROM wire_tx", tail);

    // Assert
    assert_eq!(
        result.tail[0]
            .iter()
            .map(|frame| frame.0)
            .collect::<Vec<_>>(),
        b"IZ"
    );
    assert_eq!(wire::error_code(&result.tail[1]).as_deref(), Some("26000"));
    assert_eq!(
        result.tail[1].last().map(|frame| frame.1.as_slice()),
        Some(&b"I"[..])
    );
}
