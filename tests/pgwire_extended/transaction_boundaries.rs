//! Selected transaction state laws exercised through independent wire sessions.

use super::support_pgwire as wire;
use super::support_pgwire_transaction_boundary::{run_boundary, run_cycle, run_script, Prefix};

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
fn should_roll_back_an_idle_simple_query_batch_on_error() {
    // Arrange
    let boundary = vec![wire::simple_query_frame(
        "INSERT INTO wire_tx (n) VALUES (1); SELECT n FROM wire_tx WHERE (n / 0) = 1; INSERT INTO wire_tx (n) VALUES (2)",
    )];

    // Act
    let (_, _, boundary, after) = run_cycle(boundary, "SELECT COUNT(*) FROM wire_tx", false);

    // Assert
    assert_eq!(
        boundary.iter().map(|frame| frame.0).collect::<Vec<_>>(),
        b"CEZ"
    );
    assert_eq!(wire::error_code(&boundary).as_deref(), Some("22012"));
    assert_eq!(
        boundary.last().map(|frame| frame.1.as_slice()),
        Some(&b"I"[..])
    );
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
fn should_preserve_promoted_writes_until_explicit_rollback() {
    // Arrange
    let boundary = vec![wire::simple_query_frame("BEGIN; SELECT n FROM wire_tx")];
    let tail = vec![
        vec![wire::sync_frame()],
        vec![wire::simple_query_frame("ROLLBACK")],
    ];

    // Act
    let result = run_script(
        Prefix::Implicit,
        boundary,
        "SELECT COUNT(*) FROM wire_tx",
        tail,
    );

    // Assert
    assert_eq!(
        wire::data_rows(&result.boundary),
        vec![vec![Some("1".to_string())]]
    );
    assert_eq!(
        wire::data_rows(&result.after),
        vec![vec![Some("0".to_string())]]
    );
    assert_eq!(result.tail[0], vec![(b'Z', b"T".to_vec())]);
    assert_eq!(
        result.tail[1]
            .iter()
            .map(|frame| frame.0)
            .collect::<Vec<_>>(),
        b"CZ"
    );
    assert_eq!(
        result.tail[1].last().map(|frame| frame.1.as_slice()),
        Some(&b"I"[..])
    );
    assert_eq!(
        wire::data_rows(&result.terminal),
        vec![vec![Some("0".to_string())]]
    );
}

#[test]
fn should_preserve_explicit_failed_state_until_rollback() {
    // Arrange
    let boundary = vec![wire::simple_query_frame(
        "SELECT n FROM wire_tx WHERE (n / 0) = 1",
    )];
    let tail = vec![
        vec![wire::sync_frame()],
        vec![wire::simple_query_frame("ROLLBACK")],
    ];

    // Act
    let result = run_script(
        Prefix::Explicit,
        boundary,
        "SELECT COUNT(*) FROM wire_tx",
        tail,
    );

    // Assert
    assert_eq!(wire::error_code(&result.boundary).as_deref(), Some("22012"));
    assert_eq!(
        result.boundary.last().map(|frame| frame.1.as_slice()),
        Some(&b"E"[..])
    );
    assert_eq!(result.tail[0], vec![(b'Z', b"E".to_vec())]);
    assert_eq!(
        result.tail[1].last().map(|frame| frame.1.as_slice()),
        Some(&b"I"[..])
    );
    assert_eq!(
        wire::data_rows(&result.terminal),
        vec![vec![Some("0".to_string())]]
    );
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

fn copy_cycle() -> Vec<Vec<u8>> {
    vec![
        wire::simple_query_frame("COPY wire_tx (n) FROM STDIN WITH (FORMAT csv)"),
        wire::copy_data_frame(b"2\n"),
        wire::copy_done_frame(),
    ]
}

#[test]
fn should_stage_table_copy_until_explicit_commit() {
    // Arrange
    let boundary = copy_cycle();
    let tail = vec![vec![wire::simple_query_frame("COMMIT")]];

    // Act
    let result = run_script(
        Prefix::Explicit,
        boundary,
        "SELECT n FROM wire_tx ORDER BY n",
        tail,
    );

    // Assert
    assert_eq!(wire::error_code(&result.boundary), None);
    assert_eq!(
        result
            .boundary
            .iter()
            .map(|frame| frame.0)
            .collect::<Vec<_>>(),
        b"GCZ"
    );
    assert_eq!(
        result.boundary.last().map(|frame| frame.1.as_slice()),
        Some(&b"T"[..])
    );
    assert_eq!(
        wire::data_rows(&result.after),
        Vec::<Vec<Option<String>>>::new()
    );
    assert_eq!(
        wire::data_rows(&result.terminal),
        vec![vec![Some("1".to_string())], vec![Some("2".to_string())]]
    );
}

#[test]
fn should_discard_staged_table_copy_on_explicit_rollback() {
    // Arrange
    let boundary = copy_cycle();
    let tail = vec![vec![wire::simple_query_frame("ROLLBACK")]];

    // Act
    let result = run_script(
        Prefix::Explicit,
        boundary,
        "SELECT COUNT(*) FROM wire_tx",
        tail,
    );

    // Assert
    assert_eq!(wire::error_code(&result.boundary), None);
    assert_eq!(
        result.boundary.last().map(|frame| frame.1.as_slice()),
        Some(&b"T"[..])
    );
    assert_eq!(
        wire::data_rows(&result.after),
        vec![vec![Some("0".to_string())]]
    );
    assert_eq!(
        wire::data_rows(&result.terminal),
        vec![vec![Some("0".to_string())]]
    );
}

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

#[test]
fn should_reject_savepoints_in_an_implicit_segment() {
    // Arrange
    let boundary = vec![wire::simple_query_frame("SAVEPOINT implicit_savepoint")];

    // Act
    let result = run_script(
        Prefix::Implicit,
        boundary,
        "SELECT COUNT(*) FROM wire_tx",
        Vec::new(),
    );

    // Assert
    assert_eq!(wire::error_code(&result.boundary).as_deref(), Some("0A000"));
    assert_eq!(
        result.boundary.last().map(|frame| frame.1.as_slice()),
        Some(&b"I"[..])
    );
    assert_eq!(
        wire::data_rows(&result.after),
        vec![vec![Some("0".to_string())]]
    );
}

#[test]
fn should_discard_pending_implicit_work_on_simple_query_split_error() {
    // Arrange
    let boundary = vec![wire::simple_query_frame("SELECT 'unterminated")];

    // Act
    let result = run_script(
        Prefix::Implicit,
        boundary,
        "SELECT COUNT(*) FROM wire_tx",
        Vec::new(),
    );

    // Assert
    assert_eq!(wire::error_code(&result.boundary).as_deref(), Some("42601"));
    assert_eq!(
        result.boundary.last().map(|frame| frame.1.as_slice()),
        Some(&b"I"[..])
    );
    assert_eq!(
        wire::data_rows(&result.after),
        vec![vec![Some("0".to_string())]]
    );
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

#[test]
fn should_fail_explicit_transactions_after_simple_query_output_limit_errors() {
    // Arrange
    let projections = (0..17)
        .map(|index| format!("payload AS p{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let boundary = vec![wire::simple_query_frame(&format!(
        "SELECT {projections} FROM wire_tx WHERE n = 0"
    ))];
    let tail = vec![vec![wire::simple_query_frame("ROLLBACK")]];

    // Act
    let result = run_script(
        Prefix::ExplicitOutputError,
        boundary,
        "SELECT COUNT(*) FROM wire_tx WHERE n = 1",
        tail,
    );

    // Assert
    assert_eq!(wire::error_code(&result.boundary).as_deref(), Some("54000"));
    assert_eq!(
        result
            .boundary
            .iter()
            .map(|frame| frame.0)
            .collect::<Vec<_>>(),
        b"TEZ"
    );
    assert_eq!(
        result.boundary.last().map(|frame| frame.1.as_slice()),
        Some(&b"E"[..])
    );
    assert_eq!(
        result.tail[0].last().map(|frame| frame.1.as_slice()),
        Some(&b"I"[..])
    );
    assert_eq!(
        wire::data_rows(&result.terminal),
        vec![vec![Some("0".to_string())]]
    );
}

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

#[test]
fn should_keep_nested_begin_rejected_inside_an_explicit_transaction() {
    // Arrange
    let boundary = vec![wire::simple_query_frame("BEGIN")];
    let tail = vec![vec![wire::simple_query_frame("ROLLBACK")]];

    // Act
    let result = run_script(
        Prefix::Explicit,
        boundary,
        "SELECT COUNT(*) FROM wire_tx",
        tail,
    );

    // Assert
    assert_eq!(wire::error_code(&result.boundary).as_deref(), Some("0A000"));
    assert_eq!(
        result.boundary.last().map(|frame| frame.1.as_slice()),
        Some(&b"E"[..])
    );
    assert_eq!(
        result.tail[0].last().map(|frame| frame.1.as_slice()),
        Some(&b"I"[..])
    );
    assert_eq!(
        wire::data_rows(&result.terminal),
        vec![vec![Some("0".to_string())]]
    );
}
