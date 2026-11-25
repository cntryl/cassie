#[test]
fn should_reject_copy_handoff_with_pending_implicit_work() {
    // Arrange
    let boundary = copy_cycle();
    let tail = vec![vec![wire::sync_frame()]];

    // Act
    let result = run_script(
        Prefix::Implicit,
        boundary,
        "SELECT COUNT(*) FROM wire_tx",
        tail,
    );

    // Assert
    assert_eq!(
        result
            .boundary
            .iter()
            .map(|frame| frame.0)
            .collect::<Vec<_>>(),
        b"EZ"
    );
    assert_eq!(wire::error_code(&result.boundary).as_deref(), Some("0A000"));
    assert_eq!(
        result.boundary.last().map(|frame| frame.1.as_slice()),
        Some(&b"I"[..])
    );
    assert_eq!(
        wire::data_rows(&result.after),
        vec![vec![Some("0".to_string())]]
    );
    assert_eq!(result.tail[0], vec![(b'Z', b"I".to_vec())]);
}
