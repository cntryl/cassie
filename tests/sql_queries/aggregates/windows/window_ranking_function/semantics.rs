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
