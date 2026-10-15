#[test]
fn should_preserve_the_supported_wire_isolation_boundary() {
    // Arrange
    let cases = [
        ("READ COMMITTED", true),
        ("REPEATABLE READ", false),
        ("SERIALIZABLE", false),
    ];

    for (isolation, supported) in cases {
        let boundary = vec![wire::simple_query_frame(&format!(
            "BEGIN ISOLATION LEVEL {isolation}"
        ))];
        let tail = vec![vec![wire::simple_query_frame("ROLLBACK")]];

        // Act
        let result = run_script(Prefix::Idle, boundary, "SELECT COUNT(*) FROM wire_tx", tail);

        // Assert
        if supported {
            assert_eq!(wire::error_code(&result.boundary), None);
            assert_eq!(
                result.boundary.last().map(|frame| frame.1.as_slice()),
                Some(&b"T"[..])
            );
        } else {
            assert_eq!(wire::error_code(&result.boundary).as_deref(), Some("0A000"));
            assert_eq!(
                result.boundary.last().map(|frame| frame.1.as_slice()),
                Some(&b"I"[..])
            );
        }
        assert_eq!(
            result.tail[0].last().map(|frame| frame.1.as_slice()),
            Some(&b"I"[..])
        );
        assert_eq!(
            wire::data_rows(&result.terminal),
            vec![vec![Some("0".to_string())]]
        );
    }
}
