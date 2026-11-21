//! The result-row cap bounds the rows a query returns, so rows skipped by OFFSET do not count
//! against it.
use super::support_sql_fixture::{sql_fixture_with_config, SqlFixture};
use cassie::types::Value;

fn capped_fixture(label: &str, scan_workers: usize) -> SqlFixture {
    let mut config = cassie::config::CassieRuntimeConfig::from_env().expect("runtime config");
    if config.password.is_empty() {
        crate::support_sql::test_root_password().clone_into(&mut config.password);
    }
    config.limits.max_result_rows = 10;
    config.limits.parallel_scan_workers = scan_workers;
    sql_fixture_with_config(
        label,
        &[
            "CREATE TABLE capped (n BIGINT)",
            "INSERT INTO capped VALUES (0), (1), (2), (3), (4), (5), (6), (7), (8), (9), (10), (11), \
             (12), (13), (14), (15), (16), (17), (18), (19)",
        ],
        config,
    )
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
    assert!(result.is_err(), "18 rows exceed a 10-row result cap");
}

#[test]
fn should_refuse_an_insert_whose_generated_source_exceeds_the_result_cap() {
    // Arrange
    let fixture = capped_fixture("generated-insert-cap", 1);
    fixture
        .execute("CREATE TABLE filled (n BIGINT)")
        .expect("create target");

    // Act
    let result = fixture.execute("INSERT INTO filled SELECT g FROM generate_series(0, 19) AS g");

    // Assert
    assert!(result.is_err(), "a truncated source must not be inserted");
    assert_eq!(
        fixture.rows("SELECT count(*) FROM filled"),
        vec![vec![Value::Int64(0)]]
    );
}

#[test]
fn should_insert_a_generated_source_that_fits_the_result_cap() {
    // Arrange
    let fixture = capped_fixture("generated-insert-fits", 1);
    fixture
        .execute("CREATE TABLE filled (n BIGINT)")
        .expect("create target");

    // Act
    let result = fixture.execute("INSERT INTO filled SELECT g FROM generate_series(0, 9) AS g");

    // Assert
    assert!(result.is_ok(), "ten rows fit a ten-row cap");
    assert_eq!(
        fixture.rows("SELECT count(*) FROM filled"),
        vec![vec![Value::Int64(10)]]
    );
}
