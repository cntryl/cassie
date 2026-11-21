use cassie::types::Value;

use super::support_sql_fixture::{sql_fixture, SqlFixture};

fn ordered_fixture(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TABLE rd (id INT, v INT)",
            "INSERT INTO rd VALUES (1, 1), (2, 2), (3, 3)",
        ],
    )
}

fn peer_null_fixture(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TABLE rf (id INT, v INT)",
            "INSERT INTO rf VALUES (1, 1), (2, 2), (3, 3), (4, 3), (5, NULL)",
        ],
    )
}

fn column(fixture: &SqlFixture, sql: &str) -> Vec<Option<i64>> {
    fixture
        .rows(sql)
        .into_iter()
        .map(|row| match &row[1] {
            Value::Int64(value) => Some(*value),
            Value::Null => None,
            _ => panic!("unexpected window value type for {sql}"),
        })
        .collect()
}

#[test]
fn should_count_zero_rows_when_range_end_precedes_first_row() {
    // Arrange
    let fixture = ordered_fixture("window-range-end-before-first");

    // Act
    let counts = column(
        &fixture,
        "SELECT id, count(*) OVER (ORDER BY v RANGE BETWEEN UNBOUNDED PRECEDING AND 1 PRECEDING) FROM rd ORDER BY id",
    );

    // Assert
    assert_eq!(counts, vec![Some(0), Some(1), Some(2)]);
}

#[test]
fn should_return_null_sum_when_range_frame_lies_before_every_row() {
    // Arrange
    let fixture = ordered_fixture("window-range-both-before");

    // Act
    let sums = column(
        &fixture,
        "SELECT id, sum(v) OVER (ORDER BY v RANGE BETWEEN 10 PRECEDING AND 5 PRECEDING) FROM rd ORDER BY id",
    );

    // Assert
    assert_eq!(sums, vec![None, None, None]);
}

#[test]
fn should_count_zero_rows_when_range_start_follows_last_row() {
    // Arrange
    let fixture = ordered_fixture("window-range-start-after-last");

    // Act
    let counts = column(
        &fixture,
        "SELECT id, count(*) OVER (ORDER BY v RANGE BETWEEN 1 FOLLOWING AND UNBOUNDED FOLLOWING) FROM rd ORDER BY id",
    );

    // Assert
    assert_eq!(counts, vec![Some(2), Some(1), Some(0)]);
}

#[test]
fn should_return_null_sum_when_range_frame_lies_after_every_row() {
    // Arrange
    let fixture = ordered_fixture("window-range-both-after");

    // Act
    let sums = column(
        &fixture,
        "SELECT id, sum(v) OVER (ORDER BY v RANGE BETWEEN 5 FOLLOWING AND 10 FOLLOWING) FROM rd ORDER BY id",
    );

    // Assert
    assert_eq!(sums, vec![None, None, None]);
}

#[test]
fn should_count_zero_rows_when_descending_range_end_precedes_first_row() {
    // Arrange
    let fixture = ordered_fixture("window-range-desc-end-before-first");

    // Act
    let counts = column(
        &fixture,
        "SELECT id, count(*) OVER (ORDER BY v DESC RANGE BETWEEN UNBOUNDED PRECEDING AND 1 PRECEDING) FROM rd ORDER BY id",
    );

    // Assert
    assert_eq!(counts, vec![Some(2), Some(1), Some(0)]);
}

#[test]
fn should_count_zero_rows_when_descending_range_start_follows_last_row() {
    // Arrange
    let fixture = ordered_fixture("window-range-desc-start-after-last");

    // Act
    let counts = column(
        &fixture,
        "SELECT id, count(*) OVER (ORDER BY v DESC RANGE BETWEEN 1 FOLLOWING AND UNBOUNDED FOLLOWING) FROM rd ORDER BY id",
    );

    // Assert
    assert_eq!(counts, vec![Some(0), Some(1), Some(2)]);
}

#[test]
fn should_include_leading_nulls_in_range_frame_ending_before_current_peers() {
    // Arrange
    let fixture = peer_null_fixture("window-range-nulls-first-end");

    // Act
    let counts = column(
        &fixture,
        "SELECT id, count(*) OVER (ORDER BY v NULLS FIRST RANGE BETWEEN UNBOUNDED PRECEDING AND 1 PRECEDING) FROM rf ORDER BY id",
    );

    // Assert
    assert_eq!(counts, vec![Some(1), Some(2), Some(3), Some(3), Some(1)]);
}

#[test]
fn should_include_trailing_nulls_in_range_frame_starting_after_current_peers() {
    // Arrange
    let fixture = peer_null_fixture("window-range-nulls-last-start");

    // Act
    let counts = column(
        &fixture,
        "SELECT id, count(*) OVER (ORDER BY v NULLS LAST RANGE BETWEEN 1 FOLLOWING AND UNBOUNDED FOLLOWING) FROM rf ORDER BY id",
    );

    // Assert
    assert_eq!(counts, vec![Some(4), Some(3), Some(1), Some(1), Some(1)]);
}

#[test]
fn should_count_zero_rows_when_rows_frame_lies_before_first_rows() {
    // Arrange
    let fixture = ordered_fixture("window-rows-before");

    // Act
    let counts = column(
        &fixture,
        "SELECT id, count(*) OVER (ORDER BY id ROWS BETWEEN 5 PRECEDING AND 3 PRECEDING) FROM rd ORDER BY id",
    );

    // Assert
    assert_eq!(counts, vec![Some(0), Some(0), Some(0)]);
}

#[test]
fn should_return_null_value_functions_over_empty_rows_frame() {
    // Arrange
    let fixture = ordered_fixture("window-rows-empty-values");

    // Act
    let first = column(
        &fixture,
        "SELECT id, first_value(v) OVER (ORDER BY id ROWS BETWEEN 5 PRECEDING AND 3 PRECEDING) FROM rd ORDER BY id",
    );
    let last = column(
        &fixture,
        "SELECT id, last_value(v) OVER (ORDER BY id ROWS BETWEEN 5 PRECEDING AND 3 PRECEDING) FROM rd ORDER BY id",
    );
    let nth = column(
        &fixture,
        "SELECT id, nth_value(v, 1) OVER (ORDER BY id ROWS BETWEEN 5 PRECEDING AND 3 PRECEDING) FROM rd ORDER BY id",
    );

    // Assert
    assert_eq!(
        (first, last, nth),
        (vec![None; 3], vec![None; 3], vec![None; 3])
    );
}

#[test]
fn should_count_zero_rows_when_rows_frame_lies_after_last_rows() {
    // Arrange
    let fixture = ordered_fixture("window-rows-after");

    // Act
    let counts = column(
        &fixture,
        "SELECT id, count(*) OVER (ORDER BY id ROWS BETWEEN 3 FOLLOWING AND 5 FOLLOWING) FROM rd ORDER BY id",
    );

    // Assert
    assert_eq!(counts, vec![Some(0), Some(0), Some(0)]);
}

#[test]
fn should_count_zero_rows_when_preceding_start_offset_is_smaller_than_end_offset() {
    // Arrange
    let fixture = ordered_fixture("window-rows-inverted-preceding");

    // Act
    let counts = column(
        &fixture,
        "SELECT id, count(*) OVER (ORDER BY id ROWS BETWEEN 2 PRECEDING AND 3 PRECEDING) FROM rd ORDER BY id",
    );

    // Assert
    assert_eq!(counts, vec![Some(0); 3]);
}

#[test]
fn should_count_zero_rows_when_following_start_offset_is_larger_than_end_offset() {
    // Arrange
    let fixture = ordered_fixture("window-rows-inverted-following");

    // Act
    let counts = column(
        &fixture,
        "SELECT id, count(*) OVER (ORDER BY id ROWS BETWEEN 3 FOLLOWING AND 2 FOLLOWING) FROM rd ORDER BY id",
    );

    // Assert
    assert_eq!(counts, vec![Some(0); 3]);
}

#[test]
fn should_count_peer_groups_for_groups_frame_ending_before_current_group() {
    // Arrange
    let fixture = peer_null_fixture("window-groups-preceding");

    // Act
    let counts = column(
        &fixture,
        "SELECT id, count(*) OVER (ORDER BY id GROUPS BETWEEN 2 PRECEDING AND 1 PRECEDING) FROM rf ORDER BY id",
    );

    // Assert
    assert_eq!(counts, vec![Some(0), Some(1), Some(2), Some(2), Some(2)]);
}

#[test]
fn should_count_peer_groups_for_groups_frame_after_current_group() {
    // Arrange
    let fixture = peer_null_fixture("window-groups-following");

    // Act
    let counts = column(
        &fixture,
        "SELECT id, count(*) OVER (ORDER BY v GROUPS BETWEEN 1 FOLLOWING AND 2 FOLLOWING) FROM rf ORDER BY id",
    );

    // Assert
    assert_eq!(counts, vec![Some(3), Some(3), Some(1), Some(1), Some(0)]);
}

#[test]
fn should_count_zero_rows_when_groups_frame_lies_before_first_group() {
    // Arrange
    let fixture = peer_null_fixture("window-groups-before");

    // Act
    let counts = column(
        &fixture,
        "SELECT id, count(*) OVER (ORDER BY v GROUPS BETWEEN 5 PRECEDING AND 4 PRECEDING) FROM rf ORDER BY id",
    );

    // Assert
    assert_eq!(counts, vec![Some(0); 5]);
}
