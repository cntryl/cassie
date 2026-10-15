#[test]
fn should_keep_text_parameter_literals_parseable() {
    // Arrange
    let name = "text_parameter";
    let sql = "SELECT $1 = 'maybe' AS matched";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(name, sql, &[25])),
        Step::ThroughReady(vec![wire::sync_frame()]),
    ];

    // Act
    let batches = run_script("text-comparison-literal", steps);

    // Assert
    assert_eq!(batches.len(), 2);
    assert_eq!(tags(&batches[0]), vec![b'1']);
    assert_eq!(tags(&batches[1]), vec![b'Z']);
    assert_ready(&batches[1]);
}
