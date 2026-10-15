#[test]
fn should_describe_a_typed_null_point_with_postgresql_identity() {
    // Arrange
    let sql = "SELECT NULL::point AS absent_point";

    // Act
    let frames = execute_geometry_query("issue-907-typed-null-point-identity", sql);

    // Assert
    assert_eq!(
        wire::error_code(&frames),
        None,
        "wire errors: {:?}",
        frames
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .collect::<Vec<_>>()
    );
    assert_eq!(geometry_result_types(&frames), vec![(600, 16, -1, 0)]);
    assert_eq!(wire::data_rows(&frames), vec![vec![None]]);
}
