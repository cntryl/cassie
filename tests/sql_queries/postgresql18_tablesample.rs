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
    let other_seed_sql = "SELECT id FROM issue_800_sample_rows TABLESAMPLE BERNOULLI (50) REPEATABLE (1.5) ORDER BY id";
    // PostgreSQL 18.6 returns these ids for the fixture's one-block insertion order.
    let expected_first_ids = [
        2_i64, 5, 6, 9, 10, 11, 15, 17, 18, 21, 22, 23, 24, 29, 30, 34, 35, 36, 37, 38, 45, 49, 50,
        52, 53, 55, 57, 58, 60, 61, 63, 65, 66, 67, 69, 71, 75, 77, 79, 80, 82, 83, 84, 86, 87, 89,
        90, 93, 94, 96, 100, 102, 103, 104, 105, 107, 108, 109, 110, 112, 114, 115, 122, 125, 127,
        128,
    ];
    let expected_other_seed_ids = [
        2_i64, 3, 4, 7, 9, 12, 17, 18, 25, 26, 27, 29, 34, 36, 37, 38, 39, 41, 42, 44, 45, 48, 51,
        55, 56, 57, 59, 60, 64, 65, 66, 68, 72, 74, 75, 76, 80, 82, 84, 86, 87, 91, 92, 96, 97,
        101, 104, 108, 110, 111, 112, 113, 114, 115, 117, 118, 120, 121, 125, 128,
    ];

    // Act
    let first = fixture.execute(sql);
    let second = fixture.execute(sql);
    let different_seed = fixture.execute(other_seed_sql);

    // Assert
    let first_result = first.expect("first repeatable sample");
    let second_result = second.expect("second repeatable sample");
    let different_seed_result = different_seed.expect("sample with another seed");
    let expected_first = expected_first_ids
        .into_iter()
        .map(|id| vec![Value::Int64(id)])
        .collect::<Vec<_>>();
    let expected_other_seed = expected_other_seed_ids
        .into_iter()
        .map(|id| vec![Value::Int64(id)])
        .collect::<Vec<_>>();
    assert_eq!(first_result.rows(), expected_first);
    assert_eq!(first_result.rows(), second_result.rows());
    assert_eq!(different_seed_result.rows(), expected_other_seed);
    assert_ne!(first_result.rows(), different_seed_result.rows());
}

#[test]
fn should_reproduce_system_samples_for_same_seed_with_seed_variation() {
    // Arrange
    let fixture = sample_fixture("issue-800-system-repeatable");
    let sql =
        "SELECT id FROM issue_800_sample_rows TABLESAMPLE SYSTEM (50) REPEATABLE (0.5) ORDER BY id";
    let other_seed_sql =
        "SELECT id FROM issue_800_sample_rows TABLESAMPLE SYSTEM (50) REPEATABLE (1.5) ORDER BY id";

    // Act
    let first = fixture.execute(sql);
    let second = fixture.execute(sql);
    let different_seed = fixture.execute(other_seed_sql);

    // Assert
    let first_result = first.expect("first repeatable SYSTEM sample");
    let second_result = second.expect("second repeatable SYSTEM sample");
    let different_seed_result = different_seed.expect("SYSTEM sample with another seed");
    let expected_first = (1..=128)
        .map(|id| vec![Value::Int64(id)])
        .collect::<Vec<_>>();
    assert_eq!(first_result.rows(), expected_first);
    assert_eq!(first_result.rows(), second_result.rows());
    assert!(different_seed_result.rows().is_empty());
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
fn should_apply_arithmetic_table_sample_arguments() {
    // Arrange
    let fixture = sample_fixture("issue-800-tablesample-expression-arguments");

    // Act
    let result = fixture.execute(
        "SELECT id FROM issue_800_sample_rows TABLESAMPLE BERNOULLI (100 - 0) REPEATABLE (0 + 0) ORDER BY id",
    );

    // Assert
    let expected = (1..=128)
        .map(|id| vec![Value::Int64(id)])
        .collect::<Vec<_>>();
    assert_eq!(
        result
            .expect("expression-valued TABLESAMPLE rate and seed")
            .rows(),
        expected
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
