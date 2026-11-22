//! An ADD COLUMN whose backfill fails after the schema commit leaves the column
//! durable, so the in-memory catalog must agree and a retry must resume it.
use crate::support_sql::canonical_test_collection;
use crate::support_sql_fixture::sql_fixture;
use cassie::midge::adapter::set_field_add_backfill_failure_point;
use cassie::types::Value;

const ADD: &str = "ALTER TABLE bf ADD COLUMN added INT DEFAULT 7";

fn fixture_with_failed_add(label: &str) -> crate::support_sql_fixture::SqlFixture {
    let fixture = sql_fixture(label, &["CREATE TABLE bf (keep TEXT)"]);
    let collection = canonical_test_collection(&fixture.cassie, "bf");
    let documents = (0..257)
        .map(|index| (None, serde_json::json!({"keep": format!("row-{index}")})))
        .collect();
    fixture
        .cassie
        .midge
        .put_documents(&collection, documents)
        .expect("seed more rows than one backfill batch");
    set_field_add_backfill_failure_point(true);
    assert!(fixture.execute(ADD).is_err());
    fixture
}

#[test]
fn should_register_durable_column_in_catalog_when_backfill_fails() {
    // Arrange
    let fixture = fixture_with_failed_add("add-column-backfill-catalog");

    // Act
    let selected = fixture.execute("SELECT added FROM bf LIMIT 1");

    // Assert
    assert!(selected.is_ok(), "catalog must agree with durable storage");
}

#[test]
fn should_complete_backfill_when_failed_add_column_is_retried() {
    // Arrange
    let fixture = fixture_with_failed_add("add-column-backfill-retry");

    // Act
    let retry = fixture.execute(ADD);

    // Assert
    let message = retry.as_ref().err().map(ToString::to_string);
    assert!(
        retry.is_ok(),
        "retry must resume, not report a duplicate: {message:?}"
    );
    assert_eq!(
        fixture.rows("SELECT count(*) FROM bf WHERE added = 7"),
        vec![vec![Value::Int64(257)]]
    );
}
