#[test]
fn should_commit_a_joined_query_at_completion() {
    // Arrange
    let boundary = vec![wire::simple_query_frame(
        "INSERT INTO wire_tx (n) VALUES (2); SELECT n FROM wire_tx ORDER BY n",
    )];

    // Act
    let (_, before, boundary, after) = run_boundary(boundary, "SELECT n FROM wire_tx ORDER BY n");

    // Assert
    assert_eq!(wire::data_rows(&before), vec![vec![Some("0".to_string())]]);
    assert_eq!(
        boundary.iter().map(|frame| frame.0).collect::<Vec<_>>(),
        b"CTDDCZ"
    );
    assert_eq!(wire::error_code(&boundary), None);
    assert_eq!(
        boundary.last().map(|frame| frame.1.as_slice()),
        Some(&b"I"[..])
    );
    let expected = vec![vec![Some("1".to_string())], vec![Some("2".to_string())]];
    assert_eq!(wire::data_rows(&boundary), expected);
    assert_eq!(wire::error_code(&after), None);
    assert_eq!(wire::data_rows(&after), expected);
}

#[test]
fn should_promote_joined_extended_work_on_begin() {
    // Arrange
    let boundary = vec![wire::simple_query_frame(
        "BEGIN; SELECT n FROM wire_tx ORDER BY n",
    )];

    // Act
    let (_, before, boundary, after) = run_boundary(boundary, "SELECT COUNT(*) FROM wire_tx");

    // Assert
    assert_eq!(wire::data_rows(&before), vec![vec![Some("0".to_string())]]);
    assert_eq!(wire::error_code(&boundary), None);
    assert_eq!(
        boundary.iter().map(|frame| frame.0).collect::<Vec<_>>(),
        b"CTDCZ"
    );
    assert_eq!(
        boundary.last().map(|frame| frame.1.as_slice()),
        Some(&b"T"[..])
    );
    assert_eq!(
        wire::data_rows(&boundary),
        vec![vec![Some("1".to_string())]]
    );
    assert_eq!(wire::data_rows(&after), vec![vec![Some("0".to_string())]]);
}

#[test]
fn should_accept_a_fresh_query_before_sync_after_joined_failure() {
    // Arrange
    let boundary = vec![wire::simple_query_frame(
        "SELECT n FROM wire_tx WHERE (n / 0) = 1",
    )];
    let tail = vec![
        vec![wire::simple_query_frame("SELECT 9")],
        vec![wire::sync_frame()],
    ];

    // Act
    let result = run_script(
        Prefix::Implicit,
        boundary,
        "SELECT COUNT(*) FROM wire_tx",
        tail,
    );

    // Assert
    assert_eq!(wire::error_code(&result.boundary).as_deref(), Some("22012"));
    assert_eq!(wire::error_code(&result.tail[0]), None);
    assert_eq!(
        wire::data_rows(&result.tail[0]),
        vec![vec![Some("9".to_string())]]
    );
    assert_eq!(result.tail[1], vec![(b'Z', b"I".to_vec())]);
    assert_eq!(
        wire::data_rows(&result.terminal),
        vec![vec![Some("0".to_string())]]
    );
}

#[test]
fn should_not_publish_joined_success_again_at_later_sync() {
    // Arrange
    let boundary = vec![wire::simple_query_frame(
        "INSERT INTO wire_tx (n) VALUES (2)",
    )];
    let tail = vec![vec![wire::sync_frame()]];

    // Act
    let result = run_script(
        Prefix::Implicit,
        boundary,
        "SELECT n FROM wire_tx ORDER BY n",
        tail,
    );

    // Assert
    let expected = vec![vec![Some("1".to_string())], vec![Some("2".to_string())]];
    assert_eq!(wire::data_rows(&result.after), expected);
    assert_eq!(result.tail[0], vec![(b'Z', b"I".to_vec())]);
    assert_eq!(wire::data_rows(&result.terminal), expected);
}
