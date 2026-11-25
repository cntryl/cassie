#[test]
fn should_keep_extended_writes_private_until_sync() {
    // Arrange
    let boundary = vec![wire::sync_frame()];

    // Act
    let (prefix, before, boundary, after) =
        run_boundary(boundary, "SELECT n FROM wire_tx ORDER BY n");

    // Assert
    assert_eq!(
        prefix.iter().map(|frame| frame.0).collect::<Vec<_>>(),
        b"12C"
    );
    assert_eq!(prefix[2].1, b"INSERT 0 1\0");
    assert_eq!(wire::error_code(&before), None);
    assert_eq!(wire::data_rows(&before), vec![vec![Some("0".to_string())]]);
    assert_eq!(boundary, vec![(b'Z', b"I".to_vec())]);
    assert_eq!(wire::error_code(&after), None);
    assert_eq!(wire::data_rows(&after), vec![vec![Some("1".to_string())]]);
}

#[test]
fn should_roll_back_failed_extended_work_at_sync() {
    // Arrange
    let boundary = vec![
        wire::parse_frame("failure_s", "SELECT n FROM wire_tx WHERE (n / 0) = 1"),
        wire::bind_frame("failure_p", "failure_s", &[]),
        wire::execute_frame("failure_p"),
        wire::sync_frame(),
    ];

    // Act
    let (_, before, boundary, after) = run_boundary(boundary, "SELECT COUNT(*) FROM wire_tx");

    // Assert
    assert_eq!(wire::data_rows(&before), vec![vec![Some("0".to_string())]]);
    assert_eq!(
        boundary.iter().map(|frame| frame.0).collect::<Vec<_>>(),
        b"12EZ"
    );
    assert_eq!(wire::error_code(&boundary).as_deref(), Some("22012"));
    assert_eq!(
        boundary.last().map(|frame| frame.1.as_slice()),
        Some(&b"I"[..])
    );
    assert_eq!(wire::error_code(&after), None);
    assert_eq!(wire::data_rows(&after), vec![vec![Some("0".to_string())]]);
}

#[test]
fn should_parse_the_entire_simple_query_before_executing_any_statement() {
    // Arrange
    let boundary = vec![wire::simple_query_frame(
        "INSERT INTO wire_tx (n) VALUES (1); SELECT FROM",
    )];

    // Act
    let (_, _, boundary, after) = run_cycle(boundary, "SELECT COUNT(*) FROM wire_tx", false);

    // Assert
    assert_eq!(
        boundary.iter().map(|frame| frame.0).collect::<Vec<_>>(),
        b"EZ"
    );
    assert_eq!(wire::error_code(&boundary).as_deref(), Some("42601"));
    assert_eq!(wire::data_rows(&after), vec![vec![Some("0".to_string())]]);
}

#[test]
fn should_keep_named_prepared_statements_after_idle_sync() {
    // Arrange
    let boundary = vec![
        wire::parse_frame("prepared_s", "SELECT 9"),
        wire::bind_frame("prepared_p", "prepared_s", &[]),
        wire::sync_frame(),
    ];
    let tail = vec![vec![
        wire::bind_frame("fresh_p", "prepared_s", &[]),
        wire::execute_frame("fresh_p"),
        wire::sync_frame(),
    ]];

    // Act
    let result = run_script(Prefix::Idle, boundary, "SELECT COUNT(*) FROM wire_tx", tail);

    // Assert
    assert_eq!(wire::error_code(&result.tail[0]), None);
    assert_eq!(
        wire::data_rows(&result.tail[0]),
        vec![vec![Some("9".to_string())]]
    );
    assert_eq!(
        result.tail[0].last().map(|frame| frame.1.as_slice()),
        Some(&b"I"[..])
    );
}

#[test]
fn should_keep_explicit_truncate_isolated_until_commit() {
    // Arrange
    let boundary = vec![wire::simple_query_frame("TRUNCATE TABLE wire_tx")];
    let tail = vec![vec![wire::simple_query_frame("COMMIT")]];

    // Act
    let result = run_script(
        Prefix::Explicit,
        boundary,
        "SELECT COUNT(*) FROM wire_tx",
        tail,
    );

    // Assert
    assert_eq!(wire::error_code(&result.prefix), None);
    assert_eq!(
        result
            .prefix
            .iter()
            .map(|frame| frame.0)
            .collect::<Vec<_>>(),
        b"12C"
    );
    assert_eq!(result.prefix[2].1, b"INSERT 0 1\0");
    assert_eq!(wire::error_code(&result.before), None);
    assert_eq!(
        wire::data_rows(&result.before),
        vec![vec![Some("0".to_string())]]
    );
    assert_eq!(
        wire::error_code(&result.boundary),
        None,
        "TRUNCATE in an explicit transaction should complete; frames: {:?}",
        result.boundary
    );
    assert_eq!(
        result.boundary.last().map(|frame| frame.1.as_slice()),
        Some(&b"T"[..])
    );
    assert_eq!(
        result
            .boundary
            .iter()
            .map(|frame| frame.0)
            .collect::<Vec<_>>(),
        b"CZ"
    );
    assert_eq!(result.boundary[0].1, b"TRUNCATE TABLE\0");
    assert_eq!(wire::error_code(&result.after), None);
    assert_eq!(
        wire::data_rows(&result.after),
        vec![vec![Some("0".to_string())]]
    );
    assert_eq!(wire::error_code(&result.tail[0]), None);
    assert_eq!(
        result.tail[0].last().map(|frame| frame.1.as_slice()),
        Some(&b"I"[..])
    );
    assert_eq!(wire::error_code(&result.terminal), None);
    assert_eq!(
        wire::data_rows(&result.terminal),
        vec![vec![Some("0".to_string())]]
    );
}

#[test]
fn should_finish_failed_implicit_commit_at_the_same_sync() {
    // Arrange
    let boundary = vec![wire::sync_frame()];
    let tail = vec![vec![wire::simple_query_frame("SELECT 9")]];

    // Act
    let result = run_script(
        Prefix::ImplicitParentConflict,
        boundary,
        "SELECT COUNT(*) FROM wire_tx",
        tail,
    );

    // Assert
    assert_eq!(wire::error_code(&result.boundary).as_deref(), Some("23503"));
    assert_eq!(
        result
            .boundary
            .iter()
            .map(|frame| frame.0)
            .collect::<Vec<_>>(),
        b"EZ"
    );
    assert_eq!(
        result.boundary.last().map(|frame| frame.1.as_slice()),
        Some(&b"I"[..])
    );
    assert_eq!(
        wire::data_rows(&result.after),
        vec![vec![Some("0".to_string())]]
    );
    assert_eq!(wire::error_code(&result.tail[0]), None);
    assert_eq!(
        wire::data_rows(&result.tail[0]),
        vec![vec![Some("9".to_string())]]
    );
}

#[test]
fn should_discard_simple_query_messages_after_failed_extended_work_until_sync() {
    // Arrange
    let boundary = vec![
        wire::parse_frame("failure_s", "SELECT n FROM wire_tx WHERE (n / 0) = 1"),
        wire::bind_frame("failure_p", "failure_s", &[]),
        wire::execute_frame("failure_p"),
        wire::simple_query_frame("INSERT INTO wire_tx (n) VALUES (9)"),
        wire::sync_frame(),
    ];
    let tail = vec![vec![wire::simple_query_frame("SELECT 9")]];

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
        b"12EZ"
    );
    assert_eq!(wire::error_code(&result.boundary).as_deref(), Some("22012"));
    assert_eq!(
        wire::data_rows(&result.after),
        vec![vec![Some("0".to_string())]]
    );
    assert_eq!(
        wire::data_rows(&result.tail[0]),
        vec![vec![Some("9".to_string())]]
    );
    assert_eq!(
        wire::data_rows(&result.terminal),
        vec![vec![Some("0".to_string())]]
    );
}
