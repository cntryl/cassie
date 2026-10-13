//! PostgreSQL 18 TABLESAMPLE contracts tracked by #800.

use cassie::types::Value;

use crate::support_sql_fixture::{sql_fixture, SqlFixture};

fn sample_fixture(label: &str) -> SqlFixture {
    let rows = (1..=128)
        .map(|id| format!("({id})"))
        .collect::<Vec<_>>()
        .join(", ");
    let insert = format!("INSERT INTO issue_800_sample_rows VALUES {rows}");
    sql_fixture(
        label,
        &[
            "CREATE TABLE issue_800_sample_rows (id BIGINT)",
            insert.as_str(),
        ],
    )
}

#[test]
fn should_return_no_rows_for_zero_percent_system_sampling() {
    // Arrange
    let fixture = sample_fixture("issue-800-system-sample-zero");

    // Act
    let result =
        fixture.execute("SELECT id FROM issue_800_sample_rows TABLESAMPLE SYSTEM (0) ORDER BY id");

    // Assert
    assert_eq!(
        result.expect("SYSTEM sample at zero percent").rows(),
        &[] as &[Vec<Value>]
    );
}

#[test]
fn should_return_all_rows_for_full_percent_system_sampling() {
    // Arrange
    let fixture = sample_fixture("issue-800-system-sample-full");

    // Act
    let result = fixture
        .execute("SELECT id FROM issue_800_sample_rows TABLESAMPLE SYSTEM (100) ORDER BY id");

    // Assert
    let result = result.expect("SYSTEM sample at one hundred percent");
    let rows = result.rows();
    assert_eq!(rows.len(), 128);
    assert_eq!(rows.first(), Some(&vec![Value::Int64(1)]));
    assert_eq!(rows.last(), Some(&vec![Value::Int64(128)]));
}

#[test]
fn should_return_no_rows_for_zero_percent_bernoulli_sampling() {
    // Arrange
    let fixture = sample_fixture("issue-800-bernoulli-sample-zero");

    // Act
    let result = fixture
        .execute("SELECT id FROM issue_800_sample_rows TABLESAMPLE BERNOULLI (0) ORDER BY id");

    // Assert
    assert_eq!(
        result.expect("BERNOULLI sample at zero percent").rows(),
        &[] as &[Vec<Value>]
    );
}

#[test]
fn should_return_all_rows_for_full_percent_bernoulli_sampling() {
    // Arrange
    let fixture = sample_fixture("issue-800-bernoulli-sample-full");

    // Act
    let result = fixture
        .execute("SELECT id FROM issue_800_sample_rows TABLESAMPLE BERNOULLI (100) ORDER BY id");

    // Assert
    assert_eq!(
        result
            .expect("BERNOULLI sample at one hundred percent")
            .rows()
            .len(),
        128
    );
}

#[test]
fn should_repeat_bernoulli_samples_for_the_same_seed() {
    // Arrange
    let fixture = sample_fixture("issue-800-bernoulli-repeatable");
    let sql = "SELECT id FROM issue_800_sample_rows TABLESAMPLE BERNOULLI (50) REPEATABLE (0.5) ORDER BY id";

    // Act
    let first = fixture.execute(sql);
    let second = fixture.execute(sql);

    // Assert
    assert_eq!(
        first.expect("first repeatable sample").rows(),
        second.expect("second repeatable sample").rows()
    );
}

#[test]
fn should_select_a_bounded_subset_for_intermediate_bernoulli_rates() {
    // Arrange
    let fixture = sample_fixture("issue-800-bernoulli-sample-bounds");

    // Act
    let result = fixture.execute(
        "SELECT id FROM issue_800_sample_rows TABLESAMPLE BERNOULLI (50) REPEATABLE (7.25)",
    );

    // Assert
    let sampled = result.expect("intermediate BERNOULLI sample").rows().len();
    assert!(
        (32..=96).contains(&sampled),
        "50 percent sample should be bounded away from both extremes: {sampled}"
    );
}

#[test]
fn should_accept_bound_sample_rate_seed_parameters() {
    // Arrange
    let fixture = sample_fixture("issue-800-tablesample-parameters");

    // Act
    let result = fixture.cassie.execute_sql(
        &fixture.session,
        "SELECT id FROM issue_800_sample_rows TABLESAMPLE BERNOULLI ($1) REPEATABLE ($2) ORDER BY id",
        vec![Value::Float64(100.0), Value::Float64(1.25)],
    );

    // Assert
    assert_eq!(
        result
            .expect("parameterized TABLESAMPLE rate and seed")
            .rows()
            .len(),
        128
    );
}

#[test]
fn should_reject_negative_table_sample_rates() {
    // Arrange
    let fixture = sample_fixture("issue-800-tablesample-negative-rate");

    // Act
    let result = fixture.execute("SELECT id FROM issue_800_sample_rows TABLESAMPLE BERNOULLI (-1)");

    // Assert
    assert!(
        matches!(
            result,
            Err(cassie::app::CassieError::InvalidParameterValue(_))
        ),
        "negative sampling rate must be an invalid parameter: {result:?}"
    );
}

#[test]
fn should_reject_table_sample_rates_above_one_hundred_percent() {
    // Arrange
    let fixture = sample_fixture("issue-800-tablesample-overflow-rate");

    // Act
    let result = fixture.execute("SELECT id FROM issue_800_sample_rows TABLESAMPLE SYSTEM (101)");

    // Assert
    assert!(
        matches!(
            result,
            Err(cassie::app::CassieError::InvalidParameterValue(_))
        ),
        "sampling rate above one hundred percent must be an invalid parameter: {result:?}"
    );
}

#[test]
fn should_hide_uncommitted_rows_from_other_table_sample_sessions() {
    // Arrange
    let fixture = sql_fixture(
        "issue-800-tablesample-snapshot-visibility",
        &["CREATE TABLE issue_800_sample_visibility (id BIGINT)"],
    );
    let observer = fixture.cassie.create_session("issue_800_observer", None);

    // Act
    fixture
        .cassie
        .execute_sql(&fixture.session, "BEGIN", vec![])
        .expect("begin writer transaction");
    fixture
        .cassie
        .execute_sql(
            &fixture.session,
            "INSERT INTO issue_800_sample_visibility VALUES (1)",
            vec![],
        )
        .expect("insert before sampling");
    let own_rows = fixture.cassie.execute_sql(
        &fixture.session,
        "SELECT id FROM issue_800_sample_visibility TABLESAMPLE BERNOULLI (100)",
        vec![],
    );
    let other_rows = fixture.cassie.execute_sql(
        &observer,
        "SELECT id FROM issue_800_sample_visibility TABLESAMPLE BERNOULLI (100)",
        vec![],
    );
    fixture
        .cassie
        .execute_sql(&fixture.session, "COMMIT", vec![])
        .expect("commit writer transaction");
    let committed_rows = fixture.cassie.execute_sql(
        &observer,
        "SELECT id FROM issue_800_sample_visibility TABLESAMPLE BERNOULLI (100)",
        vec![],
    );

    // Assert
    assert_eq!(
        own_rows.expect("own sampled insert").rows(),
        &[vec![Value::Int64(1)]]
    );
    assert_eq!(
        other_rows
            .expect("other session cannot sample uncommitted insert")
            .rows(),
        &[] as &[Vec<Value>]
    );
    assert_eq!(
        committed_rows
            .expect("other session samples committed insert")
            .rows(),
        &[vec![Value::Int64(1)]]
    );
}
