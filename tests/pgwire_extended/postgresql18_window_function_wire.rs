//! PostgreSQL 18 window-function values and result identities over pgwire (#806).

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

fn run_window_query(label: &str, sql: &str) -> Vec<(u8, Vec<u8>)> {
    let batches = run_script(
        label,
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_806_window_wire (id INT PRIMARY KEY, grp TEXT, n INT)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO issue_806_window_wire VALUES (1,'a',10),(2,'a',10),(3,'a',30),(4,'b',7)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(sql)]),
        ],
    );
    assert_eq!(wire::error_code(&batches[0]), None, "window fixture CREATE");
    assert_eq!(wire::error_code(&batches[1]), None, "window fixture INSERT");
    batches[2].clone()
}

fn assert_window_rows(
    frames: &[(u8, Vec<u8>)],
    expected_oids: &[i32],
    expected_rows: &[Vec<Option<String>>],
) {
    let error = frames
        .iter()
        .find(|(tag, _)| *tag == b'E')
        .map(|(_, payload)| wire::parse_error_fields(payload));
    assert_eq!(
        wire::error_code(frames),
        None,
        "window query ErrorResponse fields: {error:?}"
    );
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("window query RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        expected_oids
    );
    assert_eq!(wire::data_rows(frames), expected_rows);
}

#[test]
fn should_report_ntile_partition_buckets_with_int4_identity() {
    // Arrange
    let sql = "SELECT id, ntile(2) OVER (PARTITION BY grp ORDER BY n) \
               FROM issue_806_window_wire ORDER BY id";

    // Act
    let frames = run_window_query("issue-806-window-ntile-pgwire", sql);

    // Assert
    assert_window_rows(
        &frames,
        &[23, 23],
        &[
            vec![Some("1".to_owned()), Some("1".to_owned())],
            vec![Some("2".to_owned()), Some("1".to_owned())],
            vec![Some("3".to_owned()), Some("2".to_owned())],
            vec![Some("4".to_owned()), Some("1".to_owned())],
        ],
    );
}

#[test]
fn should_report_percent_rank_peer_values_with_float8_identity() {
    // Arrange
    let sql = "SELECT id, percent_rank() OVER (PARTITION BY grp ORDER BY n) \
               FROM issue_806_window_wire ORDER BY id";

    // Act
    let frames = run_window_query("issue-806-window-percent-rank-pgwire", sql);

    // Assert
    assert_window_rows(
        &frames,
        &[23, 701],
        &[
            vec![Some("1".to_owned()), Some("0".to_owned())],
            vec![Some("2".to_owned()), Some("0".to_owned())],
            vec![Some("3".to_owned()), Some("1".to_owned())],
            vec![Some("4".to_owned()), Some("0".to_owned())],
        ],
    );
}

#[test]
fn should_report_cume_dist_peer_values_with_float8_identity() {
    // Arrange
    let sql = "SELECT id, cume_dist() OVER (PARTITION BY grp ORDER BY n) \
               FROM issue_806_window_wire ORDER BY id";

    // Act
    let frames = run_window_query("issue-806-window-cume-dist-pgwire", sql);

    // Assert
    assert_window_rows(
        &frames,
        &[23, 701],
        &[
            vec![Some("1".to_owned()), Some("0.6666666666666666".to_owned())],
            vec![Some("2".to_owned()), Some("0.6666666666666666".to_owned())],
            vec![Some("3".to_owned()), Some("1".to_owned())],
            vec![Some("4".to_owned()), Some("1".to_owned())],
        ],
    );
}

#[test]
fn should_apply_lag_offset_defaults_as_int4_results() {
    // Arrange
    let sql = "SELECT id, lag(n, 2, -1) OVER (PARTITION BY grp ORDER BY id) \
               FROM issue_806_window_wire ORDER BY id";

    // Act
    let frames = run_window_query("issue-806-window-lag-pgwire", sql);

    // Assert
    assert_window_rows(
        &frames,
        &[23, 23],
        &[
            vec![Some("1".to_owned()), Some("-1".to_owned())],
            vec![Some("2".to_owned()), Some("-1".to_owned())],
            vec![Some("3".to_owned()), Some("10".to_owned())],
            vec![Some("4".to_owned()), Some("-1".to_owned())],
        ],
    );
}

#[test]
fn should_apply_lead_offset_defaults_as_int4_results() {
    // Arrange
    let sql = "SELECT id, lead(n, 2, -1) OVER (PARTITION BY grp ORDER BY id) \
               FROM issue_806_window_wire ORDER BY id";

    // Act
    let frames = run_window_query("issue-806-window-lead-pgwire", sql);

    // Assert
    assert_window_rows(
        &frames,
        &[23, 23],
        &[
            vec![Some("1".to_owned()), Some("30".to_owned())],
            vec![Some("2".to_owned()), Some("-1".to_owned())],
            vec![Some("3".to_owned()), Some("-1".to_owned())],
            vec![Some("4".to_owned()), Some("-1".to_owned())],
        ],
    );
}

#[test]
fn should_report_nth_value_frame_result_with_int4_identity() {
    // Arrange
    let sql = "SELECT id, nth_value(n, 2) OVER (PARTITION BY grp ORDER BY id \
               ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING) \
               FROM issue_806_window_wire ORDER BY id";

    // Act
    let frames = run_window_query("issue-806-window-nth-value-pgwire", sql);

    // Assert
    assert_window_rows(
        &frames,
        &[23, 23],
        &[
            vec![Some("1".to_owned()), Some("10".to_owned())],
            vec![Some("2".to_owned()), Some("10".to_owned())],
            vec![Some("3".to_owned()), Some("10".to_owned())],
            vec![Some("4".to_owned()), None],
        ],
    );
}
