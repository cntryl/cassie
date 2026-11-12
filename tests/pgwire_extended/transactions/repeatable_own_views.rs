//! PG18.6-pinned Bind command views survive later own writes and DDL.

use crate::support_pgwire as wire;
use crate::support_repeatable_read::{run_own_script, run_own_script_with_expected_errors};

fn query(sql: &str) -> Vec<Vec<u8>> {
    vec![wire::simple_query_frame(sql)]
}

fn ready_status(frames: &[(u8, Vec<u8>)]) -> Option<u8> {
    frames
        .iter()
        .find(|(tag, _)| *tag == b'Z')
        .and_then(|(_, body)| body.first().copied())
}

#[test]
fn should_retain_bind_own_rows_for_every_portal_page() {
    // Arrange
    let script = vec![
        query("BEGIN ISOLATION LEVEL REPEATABLE READ READ WRITE"),
        query("INSERT INTO rr_bind_rows VALUES (2)"),
        vec![
            wire::parse_frame("rows", "SELECT id FROM rr_bind_rows"),
            wire::bind_frame("bound_rows", "rows", &[]),
            wire::sync_frame(),
        ],
        query("INSERT INTO rr_bind_rows VALUES (3)"),
        vec![
            wire::execute_limited_frame("bound_rows", 1),
            wire::sync_frame(),
        ],
        query("SELECT count(*) FROM rr_bind_rows"),
        vec![
            wire::execute_limited_frame("bound_rows", 1),
            wire::sync_frame(),
        ],
        vec![wire::close_portal_frame("bound_rows"), wire::sync_frame()],
        query("ROLLBACK"),
    ];

    // Act
    let frames = run_own_script(
        &[
            "CREATE TABLE rr_bind_rows (id INT PRIMARY KEY)",
            "INSERT INTO rr_bind_rows VALUES (1)",
        ],
        script,
    );

    // Assert
    assert_eq!(wire::data_rows(&frames[4]), vec![vec![Some("1".into())]]);
    assert!(frames[4].iter().any(|(tag, _)| *tag == b's'));
    assert_eq!(wire::data_rows(&frames[5]), vec![vec![Some("3".into())]]);
    assert_eq!(wire::data_rows(&frames[6]), vec![vec![Some("2".into())]]);
}

#[test]
fn should_retain_bind_own_catalog_rows_until_execute() {
    // Arrange
    let sql = "SELECT table_name FROM information_schema.tables WHERE table_name IN ('rr_bound_catalog_a', 'rr_bound_catalog_b') ORDER BY table_name";
    let script = vec![
        query("BEGIN ISOLATION LEVEL REPEATABLE READ READ WRITE"),
        query("CREATE TABLE rr_bound_catalog_a (id INT)"),
        vec![
            wire::parse_frame("catalog", sql),
            wire::bind_frame("bound_catalog", "catalog", &[]),
            wire::sync_frame(),
        ],
        query("CREATE TABLE rr_bound_catalog_b (id INT)"),
        vec![wire::execute_frame("bound_catalog"), wire::sync_frame()],
        query(sql),
        query("ROLLBACK"),
    ];

    // Act
    let frames = run_own_script(&[], script);

    // Assert
    assert_eq!(
        wire::data_rows(&frames[4]),
        vec![vec![Some("rr_bound_catalog_a".into())]]
    );
    assert_eq!(
        wire::data_rows(&frames[5]),
        vec![
            vec![Some("rr_bound_catalog_a".into())],
            vec![Some("rr_bound_catalog_b".into())]
        ]
    );
}

#[test]
fn should_keep_an_extended_ctas_source_on_its_command_catalog() {
    // Arrange
    let sql = "CREATE TABLE rr_bound_ctas AS SELECT count(*) AS prior_count FROM information_schema.tables WHERE table_name = 'rr_bound_ctas'";
    let script = vec![
        query("BEGIN ISOLATION LEVEL REPEATABLE READ READ WRITE"),
        vec![
            wire::parse_frame("ctas", sql),
            wire::bind_frame("bound_ctas", "ctas", &[]),
            wire::sync_frame(),
        ],
        vec![wire::execute_frame("bound_ctas"), wire::sync_frame()],
        query("SELECT prior_count FROM rr_bound_ctas"),
        query("SELECT count(*) FROM information_schema.tables WHERE table_name = 'rr_bound_ctas'"),
        query("ROLLBACK"),
    ];

    // Act
    let frames = run_own_script(&[], script);

    // Assert
    assert_eq!(wire::error_code(&frames[2]), None);
    assert_eq!(
        frames[2]
            .iter()
            .find(|(tag, _)| *tag == b'C')
            .map(|(_, body)| body.as_slice()),
        Some(&b"SELECT 1\0"[..])
    );
    assert_eq!(
        frames[2].last().map(|(tag, body)| (*tag, body.as_slice())),
        Some((b'Z', &b"T"[..]))
    );
    assert_eq!(wire::data_rows(&frames[3]), vec![vec![Some("0".into())]]);
    assert_eq!(wire::data_rows(&frames[4]), vec![vec![Some("1".into())]]);
}

#[test]
fn should_keep_an_extended_ctas_bound_to_its_bind_catalog() {
    // Arrange
    let sql = "CREATE TABLE rr_bound_ctas_bind_target AS \
               SELECT count(*) AS later_count \
               FROM information_schema.tables \
               WHERE table_name = 'rr_bound_ctas_later_table'";
    let script = vec![
        query("BEGIN ISOLATION LEVEL REPEATABLE READ READ WRITE"),
        vec![
            wire::parse_frame("ctas_bind_catalog", sql),
            wire::bind_frame("bound_ctas_catalog", "ctas_bind_catalog", &[]),
            wire::sync_frame(),
        ],
        query("CREATE TABLE rr_bound_ctas_later_table (id INT)"),
        vec![
            wire::execute_frame("bound_ctas_catalog"),
            wire::sync_frame(),
        ],
        query("SELECT later_count FROM rr_bound_ctas_bind_target"),
        query(
            "SELECT count(*) FROM information_schema.tables \
             WHERE table_name = 'rr_bound_ctas_later_table'",
        ),
        query("ROLLBACK"),
    ];

    // Act
    let frames = run_own_script(&[], script);

    // Assert
    assert_eq!(wire::error_code(&frames[1]), None);
    assert_eq!(frames[1].last().map(|(tag, _)| *tag), Some(b'Z'));
    assert_eq!(
        frames[1].last().and_then(|(_, body)| body.first()),
        Some(&b'T')
    );
    assert_eq!(wire::error_code(&frames[2]), None);
    assert_eq!(wire::error_code(&frames[3]), None);
    assert_eq!(
        frames[3]
            .iter()
            .find(|(tag, _)| *tag == b'C')
            .map(|(_, body)| body.as_slice()),
        Some(&b"SELECT 1\0"[..])
    );
    assert_eq!(frames[3].last().map(|(tag, _)| *tag), Some(b'Z'));
    assert_eq!(
        frames[3].last().and_then(|(_, body)| body.first()),
        Some(&b'T')
    );
    assert_eq!(
        wire::data_rows(&frames[4]),
        vec![vec![Some("0".into())]]
    );
    assert_eq!(
        wire::data_rows(&frames[5]),
        vec![vec![Some("1".into())]]
    );
    assert_eq!(wire::error_code(&frames[6]), None);
    assert_eq!(frames[6].last().map(|(tag, _)| *tag), Some(b'Z'));
    assert_eq!(
        frames[6].last().and_then(|(_, body)| body.first()),
        Some(&b'I')
    );
}

#[test]
fn should_preserve_bind_own_rows_in_an_insert_source() {
    // Arrange
    let script = vec![
        query("BEGIN ISOLATION LEVEL REPEATABLE READ READ WRITE"),
        query("INSERT INTO rr_bind_source VALUES (2)"),
        vec![
            wire::parse_frame(
                "insert_source",
                "INSERT INTO rr_bind_target SELECT id FROM rr_bind_source RETURNING id",
            ),
            wire::bind_frame("bound_insert", "insert_source", &[]),
            wire::sync_frame(),
        ],
        query("INSERT INTO rr_bind_source VALUES (3)"),
        vec![wire::execute_frame("bound_insert"), wire::sync_frame()],
        query("SELECT count(*) FROM rr_bind_source"),
        query("ROLLBACK"),
    ];

    // Act
    let frames = run_own_script(
        &[
            "CREATE TABLE rr_bind_source (id INT PRIMARY KEY)",
            "CREATE TABLE rr_bind_target (id INT PRIMARY KEY)",
            "INSERT INTO rr_bind_source VALUES (1)",
        ],
        script,
    );
    let mut inserted = wire::data_rows(&frames[4]);
    inserted.sort();

    // Assert
    assert_eq!(
        inserted,
        vec![vec![Some("1".into())], vec![Some("2".into())]]
    );
    assert_eq!(wire::data_rows(&frames[5]), vec![vec![Some("3".into())]]);
}

#[test]
fn should_preserve_staged_sequence_advances_across_savepoint_rollback() {
    // Arrange
    for begin in [
        "BEGIN ISOLATION LEVEL READ COMMITTED",
        "BEGIN ISOLATION LEVEL REPEATABLE READ READ WRITE",
    ] {
        let script = vec![
            query(begin),
            query("CREATE TABLE staged_sequence_savepoint (id SERIAL PRIMARY KEY, payload TEXT)"),
            query("SELECT nextval('staged_sequence_savepoint_id_seq')"),
            query("SAVEPOINT sequence_point"),
            query("SELECT nextval('staged_sequence_savepoint_id_seq')"),
            query("ROLLBACK TO sequence_point"),
            query("SELECT nextval('staged_sequence_savepoint_id_seq')"),
            query("SAVEPOINT insert_point"),
            query(
                "INSERT INTO staged_sequence_savepoint(payload) VALUES ('temporary') RETURNING id",
            ),
            query("ROLLBACK TO insert_point"),
            query("SELECT nextval('staged_sequence_savepoint_id_seq')"),
            query("COMMIT"),
            query("SELECT nextval('staged_sequence_savepoint_id_seq')"),
        ];

        // Act
        let frames = run_own_script(&[], script);

        // Assert
        for (index, value) in [(2, "1"), (4, "2"), (6, "3"), (8, "4"), (10, "5"), (12, "6")] {
            assert_eq!(wire::error_code(&frames[index]), None);
            assert_eq!(
                wire::data_rows(&frames[index]),
                vec![vec![Some(value.into())]]
            );
        }
    }
}

#[test]
fn should_restart_a_rolled_back_staged_sequence_with_a_recreated_identity() {
    // Arrange
    let script = vec![
        query("BEGIN"),
        query("SAVEPOINT before_create"),
        query("CREATE TABLE staged_sequence_recreated (id SERIAL)"),
        query("SELECT nextval('staged_sequence_recreated_id_seq')"),
        query("ROLLBACK TO before_create"),
        query("CREATE TABLE staged_sequence_recreated (id SERIAL)"),
        query("SAVEPOINT before_currval"),
        query("SELECT currval('staged_sequence_recreated_id_seq')"),
        query("ROLLBACK TO before_currval"),
        query("SELECT nextval('staged_sequence_recreated_id_seq')"),
        query("ROLLBACK"),
    ];

    // Act
    let frames = run_own_script_with_expected_errors(&[], script, &[(7, "55000")]);

    // Assert
    assert_eq!(wire::error_code(&frames[7]).as_deref(), Some("55000"));
    for index in [3, 9] {
        assert_eq!(wire::error_code(&frames[index]), None);
        assert_eq!(
            wire::data_rows(&frames[index]),
            vec![vec![Some("1".into())]]
        );
    }
}

#[test]
fn should_keep_staged_sequence_advance_after_a_failed_statement() {
    // Arrange
    let script = vec![
        query("BEGIN"),
        query("CREATE TABLE staged_sequence_error (id SERIAL)"),
        query("SAVEPOINT before_error"),
        query("SELECT nextval('staged_sequence_error_id_seq') + 10/(2-n) FROM generate_series(1,2) AS s(n)"),
        query("ROLLBACK TO before_error"),
        query("SELECT currval('staged_sequence_error_id_seq')"),
        query("SELECT nextval('staged_sequence_error_id_seq')"),
        query("ROLLBACK"),
    ];

    // Act
    let frames = run_own_script_with_expected_errors(&[], script, &[(3, "22012")]);

    // Assert
    assert_eq!(wire::error_code(&frames[3]).as_deref(), Some("22012"));
    assert_eq!(wire::data_rows(&frames[5]), vec![vec![Some("2".into())]]);
    assert_eq!(wire::data_rows(&frames[6]), vec![vec![Some("3".into())]]);
}

#[test]
fn should_keep_published_sequence_advances_after_transactional_ddl_rollback() {
    // Arrange
    let script = vec![
        query("BEGIN"),
        query("CREATE TABLE staged_sequence_unrelated (n INT)"),
        query("SELECT nextval('staged_sequence_published_id_seq')"),
        query("ROLLBACK"),
        query("SELECT nextval('staged_sequence_published_id_seq')"),
    ];

    // Act
    let frames = run_own_script(
        &["CREATE TABLE staged_sequence_published (id SERIAL)"],
        script,
    );

    // Assert
    assert_eq!(wire::data_rows(&frames[2]), vec![vec![Some("1".into())]]);
    assert_eq!(wire::data_rows(&frames[4]), vec![vec![Some("2".into())]]);
}

#[test]
fn should_apply_published_serial_default_after_unrelated_ddl_in_both_isolation_levels() {
    // Arrange
    for begin in [
        "BEGIN ISOLATION LEVEL READ COMMITTED",
        "BEGIN ISOLATION LEVEL REPEATABLE READ READ WRITE",
    ] {
        let script = vec![
            query(begin),
            query("CREATE TABLE rr_published_sequence_unrelated (value INT)"),
            query("INSERT INTO rr_published_sequence_rows DEFAULT VALUES RETURNING id"),
            query(
                "SELECT last_value FROM pg_catalog.pg_sequences \
                 WHERE sequencename = 'rr_published_sequence_rows_id_seq'",
            ),
            query("CREATE TABLE rr_published_sequence_after_default (value INT)"),
            query("ROLLBACK"),
            query("SELECT count(*) FROM rr_published_sequence_rows"),
            query(
                "SELECT last_value FROM pg_catalog.pg_sequences \
                 WHERE sequencename = 'rr_published_sequence_rows_id_seq'",
            ),
            query("SELECT nextval('rr_published_sequence_rows_id_seq')"),
        ];

        // Act
        let frames = run_own_script(
            &["CREATE TABLE rr_published_sequence_rows (id SERIAL PRIMARY KEY)"],
            script,
        );

        // Assert
        for index in [0, 1, 2, 3, 4] {
            assert_eq!(wire::error_code(&frames[index]), None);
            assert_eq!(ready_status(&frames[index]), Some(b'T'));
        }
        assert_eq!(
            frames[2]
                .iter()
                .find(|(tag, _)| *tag == b'C')
                .map(|(_, body)| body.as_slice()),
            Some(&b"INSERT 0 1\0"[..])
        );
        assert_eq!(wire::data_rows(&frames[2]), vec![vec![Some("1".into())]]);
        assert_eq!(wire::data_rows(&frames[3]), vec![vec![Some("1".into())]]);
        assert_eq!(wire::error_code(&frames[4]), None);
        assert_eq!(wire::error_code(&frames[5]), None);
        assert_eq!(ready_status(&frames[5]), Some(b'I'));
        assert_eq!(wire::error_code(&frames[6]), None);
        assert_eq!(ready_status(&frames[6]), Some(b'I'));
        assert_eq!(wire::data_rows(&frames[6]), vec![vec![Some("0".into())]]);
        assert_eq!(wire::error_code(&frames[7]), None);
        assert_eq!(ready_status(&frames[7]), Some(b'I'));
        assert_eq!(wire::data_rows(&frames[7]), vec![vec![Some("1".into())]]);
        assert_eq!(wire::error_code(&frames[8]), None);
        assert_eq!(ready_status(&frames[8]), Some(b'I'));
        assert_eq!(wire::data_rows(&frames[8]), vec![vec![Some("2".into())]]);
    }
}

#[test]
fn should_commit_published_serial_default_after_repeatable_read_ddl() {
    // Arrange
    let script = vec![
        query("BEGIN ISOLATION LEVEL REPEATABLE READ READ WRITE"),
        query("CREATE TABLE rr_published_sequence_commit_unrelated (value INT)"),
        query("INSERT INTO rr_published_sequence_commit_rows DEFAULT VALUES RETURNING id"),
        query("COMMIT"),
        query("SELECT count(*) FROM rr_published_sequence_commit_rows"),
        query(
            "SELECT last_value FROM pg_catalog.pg_sequences \
             WHERE sequencename = 'rr_published_sequence_commit_rows_id_seq'",
        ),
        query("SELECT nextval('rr_published_sequence_commit_rows_id_seq')"),
    ];

    // Act
    let frames = run_own_script(
        &["CREATE TABLE rr_published_sequence_commit_rows (id SERIAL PRIMARY KEY)"],
        script,
    );

    // Assert
    for index in [0, 1, 2] {
        assert_eq!(wire::error_code(&frames[index]), None);
        assert_eq!(ready_status(&frames[index]), Some(b'T'));
    }
    assert_eq!(
        frames[2]
            .iter()
            .find(|(tag, _)| *tag == b'C')
            .map(|(_, body)| body.as_slice()),
        Some(&b"INSERT 0 1\0"[..])
    );
    assert_eq!(wire::data_rows(&frames[2]), vec![vec![Some("1".into())]]);
    for index in [3, 4, 5, 6] {
        assert_eq!(wire::error_code(&frames[index]), None);
        assert_eq!(ready_status(&frames[index]), Some(b'I'));
    }
    assert_eq!(wire::data_rows(&frames[4]), vec![vec![Some("1".into())]]);
    assert_eq!(wire::data_rows(&frames[5]), vec![vec![Some("1".into())]]);
    assert_eq!(wire::data_rows(&frames[6]), vec![vec![Some("2".into())]]);
}

#[test]
fn should_record_session_sequence_values_from_insert_defaults() {
    // Arrange
    let script = vec![
        query("BEGIN"),
        query("CREATE TABLE staged_default_currval (id SERIAL PRIMARY KEY, payload TEXT)"),
        query("INSERT INTO staged_default_currval(payload) VALUES ('first') RETURNING id"),
        query("SELECT currval('staged_default_currval_id_seq'), lastval()"),
        query("SAVEPOINT before_insert"),
        query("INSERT INTO staged_default_currval(payload) VALUES ('second')"),
        query("ROLLBACK TO before_insert"),
        query("SELECT currval('staged_default_currval_id_seq'), lastval()"),
        query("ROLLBACK"),
    ];

    // Act
    let frames = run_own_script(&[], script);

    // Assert
    assert_eq!(
        wire::data_rows(&frames[3]),
        vec![vec![Some("1".into()), Some("1".into())]]
    );
    assert_eq!(
        wire::data_rows(&frames[7]),
        vec![vec![Some("2".into()), Some("2".into())]]
    );
}

#[test]
fn should_follow_setval_for_the_last_used_sequence_identity() {
    // Arrange
    let script = vec![
        query("BEGIN"),
        query("CREATE TABLE staged_lastval (id SERIAL)"),
        query("SELECT nextval('staged_lastval_id_seq')"),
        query("SELECT setval('staged_lastval_id_seq', 7, true)"),
        query("SELECT lastval()"),
        query("SELECT setval('staged_lastval_id_seq', 11, false)"),
        query("SELECT currval('staged_lastval_id_seq'), lastval()"),
        query("ROLLBACK"),
        query("SELECT lastval()"),
    ];

    // Act
    let frames = run_own_script_with_expected_errors(&[], script, &[(8, "55000")]);

    // Assert
    assert_eq!(wire::data_rows(&frames[4]), vec![vec![Some("7".into())]]);
    assert_eq!(
        wire::data_rows(&frames[6]),
        vec![vec![Some("7".into()), Some("7".into())]]
    );
    assert_eq!(wire::error_code(&frames[8]).as_deref(), Some("55000"));
}

#[test]
fn should_restore_the_pre_restart_private_cursor_after_savepoint_rollback() {
    // Arrange
    let script = vec![
        query("BEGIN"),
        query("CREATE TABLE staged_restart_guard (id INT GENERATED BY DEFAULT AS IDENTITY)"),
        query("SELECT nextval('staged_restart_guard_id_seq')"),
        query("SAVEPOINT before_restart"),
        query("ALTER TABLE staged_restart_guard ALTER COLUMN id RESTART WITH 1"),
        query("ROLLBACK TO before_restart"),
        query("SELECT nextval('staged_restart_guard_id_seq')"),
        query("ROLLBACK"),
    ];

    // Act
    let frames = run_own_script(&[], script);

    // Assert
    assert_eq!(wire::error_code(&frames[4]), None);
    assert_eq!(wire::data_rows(&frames[6]), vec![vec![Some("2".into())]]);
}

#[test]
fn should_retain_a_dropped_private_sequence_reachable_from_a_savepoint() {
    // Arrange
    let script = vec![
        query("BEGIN"),
        query("CREATE TABLE staged_drop_recover (id SERIAL)"),
        query("SELECT nextval('staged_drop_recover_id_seq')"),
        query("SAVEPOINT before_drop"),
        query("DROP TABLE staged_drop_recover"),
        query("SELECT 1"),
        query("ROLLBACK TO before_drop"),
        query("SELECT nextval('staged_drop_recover_id_seq')"),
        query("ROLLBACK"),
    ];

    // Act
    let frames = run_own_script(&[], script);

    // Assert
    assert_eq!(wire::data_rows(&frames[2]), vec![vec![Some("1".into())]]);
    assert_eq!(wire::error_code(&frames[7]), None);
    assert_eq!(wire::data_rows(&frames[7]), vec![vec![Some("2".into())]]);
}

#[test]
fn should_restore_nested_private_sequence_restart_epochs() {
    // Arrange
    let script = vec![
        query("BEGIN"),
        query("CREATE TABLE staged_nested_restart (id INT GENERATED BY DEFAULT AS IDENTITY)"),
        query("SELECT nextval('staged_nested_restart_id_seq')"),
        query("SAVEPOINT outer_restart"),
        query("SELECT nextval('staged_nested_restart_id_seq')"),
        query("ALTER TABLE staged_nested_restart ALTER COLUMN id RESTART WITH 10"),
        query("SELECT nextval('staged_nested_restart_id_seq')"),
        query("SAVEPOINT inner_restart"),
        query("SELECT nextval('staged_nested_restart_id_seq')"),
        query("ALTER TABLE staged_nested_restart ALTER COLUMN id RESTART WITH 20"),
        query("SELECT nextval('staged_nested_restart_id_seq')"),
        query("ROLLBACK TO inner_restart"),
        query("SELECT currval('staged_nested_restart_id_seq'), lastval()"),
        query("SELECT nextval('staged_nested_restart_id_seq')"),
        query("ALTER TABLE staged_nested_restart ALTER COLUMN id RESTART WITH 30"),
        query("SELECT nextval('staged_nested_restart_id_seq')"),
        query("RELEASE inner_restart"),
        query("ROLLBACK TO outer_restart"),
        query("SELECT currval('staged_nested_restart_id_seq'), lastval()"),
        query("SELECT nextval('staged_nested_restart_id_seq')"),
        query("ROLLBACK"),
    ];

    // Act
    let frames = run_own_script(&[], script);

    // Assert
    for (index, value) in [
        (2, "1"),
        (4, "2"),
        (6, "10"),
        (8, "11"),
        (10, "20"),
        (13, "12"),
        (15, "30"),
        (19, "3"),
    ] {
        assert_eq!(wire::error_code(&frames[index]), None);
        assert_eq!(
            wire::data_rows(&frames[index]),
            vec![vec![Some(value.into())]]
        );
    }
    assert_eq!(
        wire::data_rows(&frames[12]),
        vec![vec![Some("20".into()), Some("20".into())]]
    );
    assert_eq!(
        wire::data_rows(&frames[18]),
        vec![vec![Some("30".into()), Some("30".into())]]
    );
}

#[test]
fn should_restore_the_private_cursor_after_truncate_restart_rollback() {
    // Arrange
    let script = vec![
        query("BEGIN"),
        query("CREATE TABLE staged_truncate_restart (id INT GENERATED BY DEFAULT AS IDENTITY)"),
        query("SELECT nextval('staged_truncate_restart_id_seq')"),
        query("SAVEPOINT before_truncate"),
        query("TRUNCATE staged_truncate_restart RESTART IDENTITY"),
        query("SELECT nextval('staged_truncate_restart_id_seq')"),
        query("ROLLBACK TO before_truncate"),
        query("SELECT nextval('staged_truncate_restart_id_seq')"),
        query("ROLLBACK"),
    ];

    // Act
    let frames = run_own_script(&[], script);

    // Assert
    assert_eq!(wire::error_code(&frames[4]), None);
    assert_eq!(wire::data_rows(&frames[5]), vec![vec![Some("1".into())]]);
    assert_eq!(wire::data_rows(&frames[7]), vec![vec![Some("2".into())]]);
}

#[test]
fn should_preserve_a_private_cursor_after_an_invalid_restart() {
    // Arrange
    let script = vec![
        query("BEGIN"),
        query("CREATE TABLE staged_invalid_restart (id INT GENERATED BY DEFAULT AS IDENTITY)"),
        query("SELECT nextval('staged_invalid_restart_id_seq')"),
        query("SAVEPOINT before_invalid_restart"),
        query("ALTER TABLE staged_invalid_restart ALTER COLUMN id RESTART WITH 0"),
        query("ROLLBACK TO before_invalid_restart"),
        query("SELECT nextval('staged_invalid_restart_id_seq')"),
        query("ROLLBACK"),
    ];

    // Act
    let frames = run_own_script_with_expected_errors(&[], script, &[(4, "22023")]);

    // Assert
    assert_eq!(wire::error_code(&frames[4]).as_deref(), Some("22023"));
    assert_eq!(wire::data_rows(&frames[6]), vec![vec![Some("2".into())]]);
}

#[test]
fn should_restore_private_cursor_after_sequence_option_rollback() {
    // Arrange
    let script = vec![
        query("BEGIN"),
        query("CREATE TABLE staged_epoch_options (id INT GENERATED BY DEFAULT AS IDENTITY)"),
        query("SELECT nextval('staged_epoch_options_id_seq')"),
        query("SAVEPOINT before_options"),
        query("SELECT nextval('staged_epoch_options_id_seq')"),
        query("ALTER TABLE staged_epoch_options ALTER COLUMN id SET INCREMENT BY 5"),
        query("SELECT nextval('staged_epoch_options_id_seq')"),
        query("ROLLBACK TO before_options"),
        query("SELECT nextval('staged_epoch_options_id_seq')"),
        query("ROLLBACK"),
    ];

    // Act
    let frames = run_own_script(&[], script);

    // Assert
    for (index, value) in [(2, "1"), (4, "2"), (6, "7"), (8, "3")] {
        assert_eq!(wire::error_code(&frames[index]), None);
        assert_eq!(
            wire::data_rows(&frames[index]),
            vec![vec![Some(value.into())]]
        );
    }
}
