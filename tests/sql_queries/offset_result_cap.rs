//! The result-row cap bounds the rows a query returns, so rows skipped by OFFSET do not count
//! against it.
use super::support_sql_fixture::{SqlFixture, sql_fixture_with_config};
use cassie::types::Value;

fn fixture_with_result_cap(label: &str, scan_workers: usize, max_result_rows: usize) -> SqlFixture {
    let mut config = cassie::config::CassieRuntimeConfig::from_env().expect("runtime config");
    if config.password.is_empty() {
        crate::support_sql::test_root_password().clone_into(&mut config.password);
    }
    config.limits.max_result_rows = max_result_rows;
    config.limits.parallel_scan_workers = scan_workers;
    sql_fixture_with_config(
        label,
        &[
            "CREATE TABLE capped (n BIGINT)",
            "INSERT INTO capped VALUES (1), (2), (3), (4), (5), (6), (7), (8), (9), (10), (11), (12), \
             (13), (14), (15), (16), (17), (18), (19), (20)",
        ],
        config,
    )
}

fn fixture_with_query_memory_budget(label: &str, query_memory_budget_bytes: usize) -> SqlFixture {
    let mut config = cassie::config::CassieRuntimeConfig::from_env().expect("runtime config");
    if config.password.is_empty() {
        crate::support_sql::test_root_password().clone_into(&mut config.password);
    }
    config.limits.query_memory_budget_bytes = query_memory_budget_bytes;
    sql_fixture_with_config(label, &["CREATE TABLE retained (payload TEXT)"], config)
}

fn capped_fixture(label: &str, scan_workers: usize) -> SqlFixture {
    fixture_with_result_cap(label, scan_workers, 10)
}

fn values(rows: &[Vec<Value>]) -> Vec<i64> {
    let mut values = rows
        .iter()
        .map(|row| match row[0] {
            Value::Int64(value) => value,
            _ => panic!("expected a BIGINT row"),
        })
        .collect::<Vec<_>>();
    values.sort_unstable();
    values
}

#[test]
fn should_return_the_limited_rows_when_the_offset_exceeds_the_result_cap() {
    // Arrange
    let fixture = capped_fixture("offset-cap-limit", 2);

    // Act
    let rows = fixture.rows("SELECT n FROM capped LIMIT 5 OFFSET 12");

    // Assert
    assert_eq!(rows.len(), 5);
}

#[test]
fn should_return_every_row_after_an_offset_that_exceeds_the_result_cap() {
    // Arrange
    let fixture = capped_fixture("offset-cap-open", 2);

    // Act
    let rows = fixture.rows("SELECT n FROM capped OFFSET 12");

    // Assert
    assert_eq!(rows.len(), 8);
}

#[test]
fn should_return_the_limited_rows_past_the_result_cap_with_one_scan_worker() {
    // Arrange
    let fixture = capped_fixture("offset-cap-serial", 1);

    // Act
    let rows = fixture.rows("SELECT n FROM capped LIMIT 5 OFFSET 12");

    // Assert
    assert_eq!(rows.len(), 5);
}

#[test]
fn should_return_distinct_rows_after_an_offset_within_the_result_cap() {
    // Arrange
    let fixture = capped_fixture("offset-cap-within", 2);

    // Act
    let rows = fixture.rows("SELECT n FROM capped LIMIT 5 OFFSET 3");

    // Assert
    let values = values(&rows);
    assert_eq!(values.len(), 5);
    assert!(values.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn should_still_refuse_a_result_larger_than_the_cap_after_an_offset() {
    // Arrange
    let fixture = capped_fixture("offset-cap-refused", 2);

    // Act
    let result = fixture.execute("SELECT n FROM capped OFFSET 2");

    // Assert
    assert!(
        result.is_err_and(|error| {
            error.sql_state() == "54000"
                && error
                    .to_string()
                    .contains("query result row limit exceeded")
        }),
        "18 rows exceed a 10-row result cap"
    );
}

#[test]
fn should_aggregate_a_derived_table_larger_than_the_result_cap() {
    // Arrange
    let fixture = capped_fixture("derived-aggregate-cap", 2);

    // Act
    let count = fixture.rows("SELECT count(*) FROM (SELECT n FROM capped) s");
    let sum = fixture.rows("SELECT sum(n) FROM (SELECT n FROM capped) s");
    let cte = fixture.rows("WITH c AS (SELECT n FROM capped) SELECT count(*) FROM c");

    // Assert
    assert_eq!(count, vec![vec![Value::Int64(20)]]);
    assert_eq!(sum, vec![vec![Value::Int64(210)]]);
    assert_eq!(cte, vec![vec![Value::Int64(20)]]);
}

#[test]
fn should_aggregate_an_in_subquery_larger_than_the_result_cap() {
    // Arrange
    let fixture = capped_fixture("in-subquery-cap", 2);

    // Act
    let rows = fixture.rows("SELECT count(*) FROM capped WHERE n IN (SELECT n FROM capped)");

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(20)]]);
}

#[test]
fn should_aggregate_a_joined_subquery_larger_than_the_result_cap() {
    // Arrange
    let fixture = capped_fixture("joined-subquery-cap", 2);

    // Act
    let rows =
        fixture.rows("SELECT count(*) FROM capped a JOIN (SELECT n FROM capped) b ON a.n = b.n");

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(20)]]);
}

#[test]
fn should_insert_every_row_from_a_derived_source_larger_than_the_result_cap() {
    // Arrange
    let fixture = capped_fixture("derived-insert-cap", 1);
    fixture
        .execute("CREATE TABLE filled (n BIGINT)")
        .expect("create target");

    // Act
    let result = fixture.execute("INSERT INTO filled SELECT n FROM (SELECT n FROM capped) s");

    // Assert
    assert!(result.is_ok(), "all source rows fit the memory budget");
    assert_eq!(
        fixture.rows("SELECT count(*) FROM filled"),
        vec![vec![Value::Int64(20)]]
    );
}

#[test]
fn should_update_every_matching_row_from_a_derived_source_larger_than_the_result_cap() {
    // Arrange
    let fixture = capped_fixture("derived-update-cap", 1);
    fixture
        .execute("CREATE TABLE updated (n BIGINT, hit BIGINT)")
        .expect("create target");
    fixture
        .execute("INSERT INTO updated SELECT n, 0 FROM capped")
        .expect("seed target");

    // Act
    let result = fixture
        .execute("UPDATE updated SET hit = 1 FROM (SELECT n FROM capped) s WHERE updated.n = s.n");

    // Assert
    assert!(result.is_ok(), "all source rows fit the memory budget");
    assert_eq!(
        fixture.rows("SELECT count(*) FROM updated WHERE hit = 1"),
        vec![vec![Value::Int64(20)]]
    );
}

#[test]
fn should_update_every_matching_row_from_a_direct_source_larger_than_the_result_cap() {
    // Arrange
    let fixture = capped_fixture("direct-update-source-cap", 1);
    fixture
        .execute("CREATE TABLE updated (n BIGINT, hit BIGINT)")
        .expect("create target");
    fixture
        .execute("INSERT INTO updated SELECT n, 0 FROM capped")
        .expect("seed target");

    // Act
    let result = fixture.execute("UPDATE updated SET hit = 1 FROM capped s WHERE updated.n = s.n");

    // Assert
    assert!(
        result.is_ok(),
        "all direct source rows fit the memory budget"
    );
    assert_eq!(
        fixture.rows("SELECT count(*) FROM updated WHERE hit = 1"),
        vec![vec![Value::Int64(20)]]
    );
}

#[test]
fn should_delete_every_matching_row_from_a_direct_source_larger_than_the_result_cap() {
    // Arrange
    let fixture = capped_fixture("direct-delete-source-cap", 1);
    fixture
        .execute("CREATE TABLE deleted (n BIGINT)")
        .expect("create target");
    fixture
        .execute("INSERT INTO deleted SELECT n FROM capped")
        .expect("seed target");

    // Act
    let result = fixture.execute("DELETE FROM deleted USING capped s WHERE deleted.n = s.n");

    // Assert
    assert!(
        result.is_ok(),
        "all direct source rows fit the memory budget"
    );
    assert_eq!(
        fixture.rows("SELECT count(*) FROM deleted"),
        vec![vec![Value::Int64(0)]]
    );
}

#[test]
fn should_merge_every_matching_row_from_a_direct_source_larger_than_the_result_cap() {
    // Arrange
    let fixture = capped_fixture("direct-merge-source-cap", 1);
    fixture
        .execute("CREATE TABLE merged (n BIGINT, hit BIGINT)")
        .expect("create target");
    fixture
        .execute("INSERT INTO merged SELECT n, 0 FROM capped")
        .expect("seed target");

    // Act
    let result = fixture.execute(
        "MERGE INTO merged t USING capped s ON t.n = s.n WHEN MATCHED THEN UPDATE SET hit = 1",
    );

    // Assert
    assert!(
        result.is_ok(),
        "all direct source rows fit the memory budget"
    );
    assert_eq!(
        fixture.rows("SELECT count(*) FROM merged WHERE hit = 1"),
        vec![vec![Value::Int64(20)]]
    );
}

#[test]
fn should_consume_over_cap_data_modifying_cte_returning_rows_as_an_input() {
    // Arrange
    let fixture = capped_fixture("modifying-cte-returning-cap", 1);
    fixture
        .execute("CREATE TABLE inserted (n BIGINT)")
        .expect("create target");

    // Act
    let rows = fixture.rows(
        "WITH inserted_rows AS (INSERT INTO inserted SELECT n FROM capped RETURNING n) \
         SELECT count(*) FROM inserted_rows",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(20)]]);
    assert_eq!(
        fixture.rows("SELECT count(*) FROM inserted"),
        vec![vec![Value::Int64(20)]]
    );
}

#[test]
fn should_rollback_modifying_cte_when_query_memory_budget_is_exceeded() {
    // Arrange
    let fixture = fixture_with_query_memory_budget("modifying-cte-copy-budget", 8 * 1024);

    // Act
    let result = fixture.execute(
        "WITH inserted AS (INSERT INTO retained VALUES (repeat('x', 4096)) RETURNING payload) \
         SELECT length(payload) FROM inserted",
    );

    // Assert
    assert!(
        result.is_err_and(|error| {
            error.sql_state() == "54000" && error.to_string().contains("query memory budget")
        }),
        "the query must be rejected by the memory budget"
    );
    assert_eq!(
        fixture.rows("SELECT count(*) FROM retained"),
        vec![vec![Value::Int64(0)]],
        "the budget denial must roll back the modifying CTE"
    );
}

#[test]
fn should_execute_modifying_cte_returning_when_retained_copies_fit_the_budget() {
    // Arrange
    // Source unit tests cover clone admission; this end-to-end query fits
    // within 256 KiB, while the 8 KiB case verifies rollback after denial.
    let fixture = fixture_with_query_memory_budget("modifying-cte-copy-budget-fit", 256 * 1024);

    // Act
    let result = fixture.rows(
        "WITH inserted AS (INSERT INTO retained VALUES (repeat('x', 4096)) RETURNING payload) \
         SELECT length(payload) FROM inserted",
    );

    // Assert
    assert_eq!(result, vec![vec![Value::Int64(4096)]]);
    assert_eq!(
        fixture.rows("SELECT count(*) FROM retained"),
        vec![vec![Value::Int64(1)]]
    );
}

#[test]
fn should_allow_empty_result_at_zero_row_cap() {
    // Arrange
    let fixture = fixture_with_result_cap("zero-result-cap-scalar", 1, 0);

    // Act
    let rows = fixture.rows("SELECT n FROM capped WHERE n = (SELECT 100)");

    // Assert
    assert_eq!(rows, [] as [Vec<Value>; 0]);
}

#[test]
fn should_insert_every_generated_source_row_under_the_result_cap() {
    // Arrange
    let fixture = capped_fixture("generated-insert-all", 1);
    fixture
        .execute("CREATE TABLE filled (n BIGINT)")
        .expect("create target");

    // Act
    let result = fixture.execute("INSERT INTO filled SELECT g FROM generate_series(0, 19) AS g");

    // Assert
    assert!(result.is_ok(), "all generated source rows must be supplied");
    assert_eq!(
        fixture.rows("SELECT count(*) FROM filled"),
        vec![vec![Value::Int64(20)]]
    );
}

#[test]
fn should_reject_over_cap_returning_before_publishing_inserted_rows() {
    // Arrange
    let fixture = capped_fixture("returning-cap-atomic", 1);
    fixture
        .execute("CREATE TABLE returned (n BIGINT)")
        .expect("create target");

    // Act
    let error = fixture.error("INSERT INTO returned SELECT n FROM capped WHERE n < 12 RETURNING n");

    // Assert
    assert_eq!(error.sql_state(), "54000");
    assert!(
        error
            .to_string()
            .contains("query result row limit exceeded")
    );
    assert_eq!(
        fixture.rows("SELECT count(*) FROM returned"),
        vec![vec![Value::Int64(0)]],
    );
}
