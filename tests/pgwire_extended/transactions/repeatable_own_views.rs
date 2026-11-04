//! PG18.6-pinned Bind command views survive later own writes and DDL.

use crate::support_pgwire as wire;
use crate::support_repeatable_read::run_own_script;

fn query(sql: &str) -> Vec<Vec<u8>> {
    vec![wire::simple_query_frame(sql)]
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
    assert_eq!(wire::data_rows(&frames[3]), vec![vec![Some("0".into())]]);
    assert_eq!(wire::data_rows(&frames[4]), vec![vec![Some("1".into())]]);
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
