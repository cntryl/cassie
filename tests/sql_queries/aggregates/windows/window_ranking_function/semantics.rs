use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

fn ranking_fixture(label: &str) -> crate::support_sql_fixture::SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TABLE issue_806_window_ranks (id INT PRIMARY KEY, grp TEXT, n INT)",
            "INSERT INTO issue_806_window_ranks VALUES (1,'a',10),(2,'a',10),(3,'a',30),(4,'b',7)",
        ],
    )
}

#[test]
fn should_distribute_ntile_buckets_within_each_partition() {
    // Arrange
    let fixture = ranking_fixture("issue-806-window-ntile-partitions");

    // Act
    let result = fixture.execute(
        "SELECT id, ntile(2) OVER (PARTITION BY grp ORDER BY n) FROM issue_806_window_ranks ORDER BY id",
    );

    // Assert
    assert_eq!(
        result.expect("NTILE partitions").rows(),
        &[
            vec![Value::Int64(1), Value::Int64(1)],
            vec![Value::Int64(2), Value::Int64(1)],
            vec![Value::Int64(3), Value::Int64(2)],
            vec![Value::Int64(4), Value::Int64(1)],
        ]
    );
}

#[test]
fn should_calculate_percent_rank_from_peer_group_positions() {
    // Arrange
    let fixture = ranking_fixture("issue-806-window-percent-rank-peers");

    // Act
    let result = fixture.execute(
        "SELECT id, percent_rank() OVER (PARTITION BY grp ORDER BY n) FROM issue_806_window_ranks ORDER BY id",
    );

    // Assert
    assert_eq!(
        result.expect("percent_rank peer positions").rows(),
        &[
            vec![Value::Int64(1), Value::Float64(0.0)],
            vec![Value::Int64(2), Value::Float64(0.0)],
            vec![Value::Int64(3), Value::Float64(1.0)],
            vec![Value::Int64(4), Value::Float64(0.0)],
        ]
    );
}

#[test]
fn should_calculate_cume_dist_for_peer_groups() {
    // Arrange
    let fixture = ranking_fixture("issue-806-window-cume-dist-peers");

    // Act
    let result = fixture.execute(
        "SELECT id, cume_dist() OVER (PARTITION BY grp ORDER BY n) FROM issue_806_window_ranks ORDER BY id",
    );

    // Assert
    assert_eq!(
        result.expect("cume_dist peer groups").rows(),
        &[
            vec![Value::Int64(1), Value::Float64(2.0 / 3.0)],
            vec![Value::Int64(2), Value::Float64(2.0 / 3.0)],
            vec![Value::Int64(3), Value::Float64(1.0)],
            vec![Value::Int64(4), Value::Float64(1.0)],
        ]
    );
}

#[test]
fn should_apply_lag_offset_default_at_partition_edges() {
    // Arrange
    let fixture = ranking_fixture("issue-806-window-lag-offset-default");

    // Act
    let result = fixture.execute(
        "SELECT id, lag(n, 2, -1) OVER (PARTITION BY grp ORDER BY id) FROM issue_806_window_ranks ORDER BY id",
    );

    // Assert
    assert_eq!(
        result.expect("lag offset with default").rows(),
        &[
            vec![Value::Int64(1), Value::Int64(-1)],
            vec![Value::Int64(2), Value::Int64(-1)],
            vec![Value::Int64(3), Value::Int64(10)],
            vec![Value::Int64(4), Value::Int64(-1)],
        ]
    );
}

#[test]
fn should_apply_lead_offset_default_at_partition_edges() {
    // Arrange
    let fixture = ranking_fixture("issue-806-window-lead-offset-default");

    // Act
    let result = fixture.execute(
        "SELECT id, lead(n, 2, -1) OVER (PARTITION BY grp ORDER BY id) FROM issue_806_window_ranks ORDER BY id",
    );

    // Assert
    assert_eq!(
        result.expect("lead offset with default").rows(),
        &[
            vec![Value::Int64(1), Value::Int64(30)],
            vec![Value::Int64(2), Value::Int64(-1)],
            vec![Value::Int64(3), Value::Int64(-1)],
            vec![Value::Int64(4), Value::Int64(-1)],
        ]
    );
}

#[test]
fn should_ignore_current_frame_for_lag_lead() {
    // Arrange
    let fixture = ranking_fixture("issue-806-window-offsets-ignore-frame");

    // Act
    let result = fixture.execute(
        "SELECT id, lag(n) OVER (PARTITION BY grp ORDER BY id ROWS BETWEEN CURRENT ROW AND CURRENT ROW), lead(n) OVER (PARTITION BY grp ORDER BY id ROWS BETWEEN CURRENT ROW AND CURRENT ROW) FROM issue_806_window_ranks ORDER BY id",
    );

    // Assert
    assert_eq!(
        result.expect("LAG and LEAD use partition positions").rows(),
        &[
            vec![Value::Int64(1), Value::Null, Value::Int64(10)],
            vec![Value::Int64(2), Value::Int64(10), Value::Int64(30)],
            vec![Value::Int64(3), Value::Int64(10), Value::Null],
            vec![Value::Int64(4), Value::Null, Value::Null],
        ]
    );
}

#[test]
fn should_apply_negative_lag_offset() {
    // Arrange
    let fixture = ranking_fixture("issue-806-window-negative-lag");

    // Act
    let result = fixture.execute(
        "SELECT id, lag(n, -1) OVER (PARTITION BY grp ORDER BY id) FROM issue_806_window_ranks ORDER BY id",
    );

    // Assert
    assert_eq!(
        result.expect("negative lag offset").rows(),
        &[
            vec![Value::Int64(1), Value::Int64(10)],
            vec![Value::Int64(2), Value::Int64(30)],
            vec![Value::Int64(3), Value::Null],
            vec![Value::Int64(4), Value::Null],
        ]
    );
}

#[test]
fn should_apply_negative_lead_offset() {
    // Arrange
    let fixture = ranking_fixture("issue-806-window-negative-lead");

    // Act
    let result = fixture.execute(
        "SELECT id, lead(n, -1) OVER (PARTITION BY grp ORDER BY id) FROM issue_806_window_ranks ORDER BY id",
    );

    // Assert
    assert_eq!(
        result.expect("negative lead offset").rows(),
        &[
            vec![Value::Int64(1), Value::Null],
            vec![Value::Int64(2), Value::Int64(10)],
            vec![Value::Int64(3), Value::Int64(10)],
            vec![Value::Int64(4), Value::Null],
        ]
    );
}

#[test]
fn should_evaluate_lag_default_against_current_row() {
    // Arrange
    let fixture = ranking_fixture("issue-806-window-current-row-default");

    // Act
    let result = fixture.execute(
        "SELECT id, lag(n, 2, id * -1) OVER (PARTITION BY grp ORDER BY id) FROM issue_806_window_ranks ORDER BY id",
    );

    // Assert
    assert_eq!(
        result.expect("lag default against current row").rows(),
        &[
            vec![Value::Int64(1), Value::Int64(-1)],
            vec![Value::Int64(2), Value::Int64(-2)],
            vec![Value::Int64(3), Value::Int64(10)],
            vec![Value::Int64(4), Value::Int64(-4)],
        ]
    );
}

#[test]
fn should_evaluate_lead_default_lazily_against_current_row() {
    // Arrange
    let fixture = ranking_fixture("issue-806-window-lead-lazy-default");

    // Act
    let result = fixture.execute(
        "SELECT id, lead(n, 2, id * -1) OVER (PARTITION BY grp ORDER BY id), lead(n, 0, id / (id - id)) OVER (PARTITION BY grp ORDER BY id), lead(n, NULL, id / (id - id)) OVER (PARTITION BY grp ORDER BY id) FROM issue_806_window_ranks ORDER BY id",
    );

    // Assert
    assert_eq!(
        result
            .expect("LEAD evaluates only the needed current-row default")
            .rows(),
        &[
            vec![
                Value::Int64(1),
                Value::Int64(30),
                Value::Int64(10),
                Value::Null,
            ],
            vec![
                Value::Int64(2),
                Value::Int64(-2),
                Value::Int64(10),
                Value::Null,
            ],
            vec![
                Value::Int64(3),
                Value::Int64(-3),
                Value::Int64(30),
                Value::Null,
            ],
            vec![
                Value::Int64(4),
                Value::Int64(-4),
                Value::Int64(7),
                Value::Null,
            ],
        ]
    );
}

#[test]
fn should_skip_lag_default_when_target_exists_or_offset_is_null() {
    // Arrange
    let fixture = ranking_fixture("issue-806-window-lazy-default");

    // Act
    let result = fixture.execute(
        "SELECT id, lag(n, 0, id / (id - id)) OVER (PARTITION BY grp ORDER BY id), lag(n, NULL, id / (id - id)) OVER (PARTITION BY grp ORDER BY id) FROM issue_806_window_ranks ORDER BY id",
    );

    // Assert
    assert_eq!(
        result
            .expect("unused or bypassed lag default is not evaluated")
            .rows(),
        &[
            vec![Value::Int64(1), Value::Int64(10), Value::Null],
            vec![Value::Int64(2), Value::Int64(10), Value::Null],
            vec![Value::Int64(3), Value::Int64(30), Value::Null],
            vec![Value::Int64(4), Value::Int64(7), Value::Null],
        ]
    );
}

#[test]
fn should_return_null_for_null_nth_value_position() {
    // Arrange
    let fixture = ranking_fixture("issue-806-window-null-nth-position");

    // Act
    let result = fixture.execute(
        "SELECT id, nth_value(n, NULL) OVER (PARTITION BY grp ORDER BY id ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING) FROM issue_806_window_ranks ORDER BY id",
    );

    // Assert
    assert_eq!(
        result.expect("NULL NTH_VALUE position").rows(),
        &[
            vec![Value::Int64(1), Value::Null],
            vec![Value::Int64(2), Value::Null],
            vec![Value::Int64(3), Value::Null],
            vec![Value::Int64(4), Value::Null],
        ]
    );
}

#[test]
fn should_return_nth_value_from_the_full_window_frame() {
    // Arrange
    let fixture = ranking_fixture("issue-806-window-nth-value-full-frame");

    // Act
    let result = fixture.execute(
        "SELECT id, nth_value(n, 2) OVER (PARTITION BY grp ORDER BY id ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING) FROM issue_806_window_ranks ORDER BY id",
    );

    // Assert
    assert_eq!(
        result.expect("nth_value over full partition frame").rows(),
        &[
            vec![Value::Int64(1), Value::Int64(10)],
            vec![Value::Int64(2), Value::Int64(10)],
            vec![Value::Int64(3), Value::Int64(10)],
            vec![Value::Int64(4), Value::Null],
        ]
    );
}

#[test]
fn should_evaluate_nth_value_relative_to_sliding_frame() {
    // Arrange
    let fixture = ranking_fixture("issue-806-window-nth-value-sliding-frame");

    // Act
    let result = fixture.execute(
        "SELECT id, nth_value(n, 2) OVER (PARTITION BY grp ORDER BY id ROWS BETWEEN 1 PRECEDING AND 1 FOLLOWING) FROM issue_806_window_ranks ORDER BY id",
    );

    // Assert
    assert_eq!(
        result
            .expect("nth_value uses each row's current frame")
            .rows(),
        &[
            vec![Value::Int64(1), Value::Int64(10)],
            vec![Value::Int64(2), Value::Int64(10)],
            vec![Value::Int64(3), Value::Int64(30)],
            vec![Value::Int64(4), Value::Null],
        ]
    );
}

#[test]
fn should_count_null_values_in_nth_value_frame() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-window-nth-value-null-row",
        &[
            "CREATE TABLE issue_806_window_nth_nulls (id INT PRIMARY KEY, n INT)",
            "INSERT INTO issue_806_window_nth_nulls VALUES (1,10),(2,NULL),(3,30)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT id, nth_value(n, 2) OVER (ORDER BY id ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING) FROM issue_806_window_nth_nulls ORDER BY id",
    );

    // Assert
    assert_eq!(
        result
            .expect("NTH_VALUE respects NULL value positions")
            .rows(),
        &[
            vec![Value::Int64(1), Value::Null],
            vec![Value::Int64(2), Value::Null],
            vec![Value::Int64(3), Value::Null],
        ]
    );
}
