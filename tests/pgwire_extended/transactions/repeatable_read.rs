//! PG18.6-pinned first-snapshot boundaries in explicit REPEATABLE READ.

use crate::support_pgwire as wire;
use crate::support_repeatable_read::{assert_counts, run_boundary};

const COUNT: &str = "SELECT count(*) FROM rr_boundary_rows";

fn execute_prepared() -> Vec<Vec<u8>> {
    vec![
        wire::bind_frame("", "rr", &[]),
        wire::execute_frame(""),
        wire::sync_frame(),
    ]
}

#[test]
fn should_capture_repeatable_read_at_constant_select() {
    // Arrange
    let boundary = vec![wire::simple_query_frame("SELECT 1")];
    // Act
    let result = run_boundary(Vec::new(), boundary, vec![wire::simple_query_frame(COUNT)]);
    // Assert
    assert_counts(&result, "1");
}

#[test]
fn should_capture_repeatable_read_at_parse_in_explicit_transaction() {
    // Arrange
    let boundary = vec![wire::parse_frame("rr", COUNT), wire::sync_frame()];
    // Act
    let result = run_boundary(Vec::new(), boundary, execute_prepared());
    // Assert
    assert_counts(&result, "1");
}

#[test]
fn should_capture_repeatable_read_at_bind_in_explicit_transaction() {
    // Arrange
    let prefix = vec![wire::parse_frame("rr", COUNT), wire::sync_frame()];
    let boundary = vec![wire::bind_frame("", "rr", &[]), wire::sync_frame()];
    // Act
    let result = run_boundary(
        prefix,
        boundary,
        vec![wire::execute_frame(""), wire::sync_frame()],
    );
    // Assert
    assert_counts(&result, "1");
}

#[test]
fn should_leave_repeatable_read_uncaptured_at_describe() {
    // Arrange
    let prefix = vec![wire::parse_frame("rr", COUNT), wire::sync_frame()];
    let boundary = vec![wire::describe_statement_frame("rr"), wire::sync_frame()];
    // Act
    let result = run_boundary(prefix, boundary, execute_prepared());
    // Assert
    assert_counts(&result, "2");
}

#[test]
fn should_leave_repeatable_read_uncaptured_at_nonsnapshot_controls() {
    // Arrange
    let boundary = vec![wire::simple_query_frame(
        "SHOW transaction_isolation; SAVEPOINT before_view",
    )];
    // Act
    let result = run_boundary(Vec::new(), boundary, vec![wire::simple_query_frame(COUNT)]);
    // Assert
    assert_counts(&result, "2");
}

#[test]
fn should_capture_repeatable_read_before_nested_session_utilities() {
    // Arrange
    let utilities = [
        "PREPARE rr_sql AS SELECT 1",
        "DECLARE rr_cursor CURSOR FOR SELECT 1",
        "EXPLAIN SELECT 1",
    ];
    for sql in utilities {
        // Act
        let result = run_boundary(
            Vec::new(),
            vec![wire::simple_query_frame(sql)],
            vec![wire::simple_query_frame(COUNT)],
        );
        // Assert
        assert_counts(&result, "1");
    }
}

#[test]
fn should_capture_repeatable_read_at_deallocate() {
    // Arrange
    let prefix = vec![wire::simple_query_frame("PREPARE rr_sql AS SELECT 1")];
    let boundary = vec![wire::simple_query_frame("DEALLOCATE rr_sql")];
    // Act
    let result = run_boundary(prefix, boundary, vec![wire::simple_query_frame(COUNT)]);
    // Assert
    assert_counts(&result, "1");
}

#[test]
fn should_retain_repeatable_read_for_suspended_direct_cursor_pages() {
    // Arrange
    let prefix = vec![
        vec![wire::simple_query_frame(
            "INSERT INTO rr_boundary_rows VALUES (2)",
        )],
        vec![
            wire::parse_frame("rr", "SELECT id FROM rr_boundary_rows"),
            wire::sync_frame(),
        ],
    ];
    let boundary = vec![wire::bind_frame("rr_portal", "rr", &[]), wire::sync_frame()];
    let pages = (0..3)
        .map(|_| {
            vec![
                wire::execute_limited_frame("rr_portal", 1),
                wire::sync_frame(),
            ]
        })
        .collect();

    // Act
    let (pages, fresh) = crate::support_repeatable_read::run_script(prefix, boundary, pages);

    // Assert
    assert_eq!(wire::data_rows(&pages[0]), vec![vec![Some("1".to_owned())]]);
    assert!(pages[0].iter().any(|(tag, _)| *tag == b's'));
    assert_eq!(wire::data_rows(&pages[1]), vec![vec![Some("2".to_owned())]]);
    assert_eq!(
        wire::data_rows(&pages[2]),
        Vec::<Vec<Option<String>>>::new()
    );
    assert_eq!(wire::data_rows(&fresh), vec![vec![Some("3".to_owned())]]);
}

#[test]
fn should_release_repeatable_read_charge_when_the_connection_terminates() {
    // Arrange
    // The fixture owns its listener and observes the first query's tracker.

    // Act
    let (retained, released) = crate::support_repeatable_read::disconnect_retained_view();

    // Assert
    assert!(retained > 0);
    assert_eq!(released, 0);
}
