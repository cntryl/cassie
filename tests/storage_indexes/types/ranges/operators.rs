//! Range predicates and indexed range access paths (#905).

use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[test]
fn should_match_scan_results_given_indexed_range_values_when_range_predicates_execute() {
    // Arrange
    let fixture = sql_fixture("issue-905-indexed-range-predicates", &[]);
    for sql in [
        "CREATE TABLE issue_905_range_scan (id INT4, value INT4RANGE)",
        "CREATE TABLE issue_905_range_indexed (id INT4, value INT4RANGE)",
        "INSERT INTO issue_905_range_scan VALUES \
         (1, '[1,4)'), (2, '[4,8)'), (3, '[8,12)'), (4, 'empty')",
        "INSERT INTO issue_905_range_indexed SELECT id, value FROM issue_905_range_scan",
        "CREATE INDEX issue_905_range_indexed_gist \
         ON issue_905_range_indexed USING GIST (value)",
    ] {
        fixture
            .execute(sql)
            .unwrap_or_else(|error| panic!("PostgreSQL-valid range setup failed: {sql}: {error}"));
    }

    // Act
    let scan_contains =
        fixture.execute("SELECT id FROM issue_905_range_scan WHERE value @> 5 ORDER BY id");
    let indexed_contains =
        fixture.execute("SELECT id FROM issue_905_range_indexed WHERE value @> 5 ORDER BY id");
    let scan_overlap = fixture.execute(
        "SELECT id FROM issue_905_range_scan \
         WHERE value && CAST('[3,9)' AS INT4RANGE) ORDER BY id",
    );
    let indexed_overlap = fixture.execute(
        "SELECT id FROM issue_905_range_indexed \
         WHERE value && CAST('[3,9)' AS INT4RANGE) ORDER BY id",
    );

    // Assert
    let contains = &[vec![Value::Int64(2)]];
    let overlap = &[
        vec![Value::Int64(1)],
        vec![Value::Int64(2)],
        vec![Value::Int64(3)],
    ];
    assert_eq!(
        scan_contains.expect("range containment scan").rows(),
        contains
    );
    assert_eq!(
        indexed_contains.expect("indexed range containment").rows(),
        contains
    );
    assert_eq!(scan_overlap.expect("range overlap scan").rows(), overlap);
    assert_eq!(
        indexed_overlap.expect("indexed range overlap").rows(),
        overlap
    );
}

#[test]
fn should_match_scan_results_given_a_custom_range_with_subtype_diff_when_its_index_is_queried() {
    // Arrange
    let fixture = sql_fixture("issue-905-indexed-custom-range-subtype-diff", &[]);
    fixture
        .execute(
            "CREATE TYPE issue_905_indexed_float_range AS RANGE \
             (subtype = FLOAT8, subtype_diff = pg_catalog.float8mi)",
        )
        .expect("create the PostgreSQL custom range and subtype-difference routine");
    for sql in [
        "CREATE TABLE issue_905_custom_range_scan \
         (id INT4, value issue_905_indexed_float_range)",
        "CREATE TABLE issue_905_custom_range_indexed \
         (id INT4, value issue_905_indexed_float_range)",
        "INSERT INTO issue_905_custom_range_scan VALUES \
         (1, '[1,2]'), (2, '[2,4]'), (3, '[4,6]')",
        "INSERT INTO issue_905_custom_range_indexed \
         SELECT id, value FROM issue_905_custom_range_scan",
        "CREATE INDEX issue_905_custom_range_gist \
         ON issue_905_custom_range_indexed USING GIST (value)",
    ] {
        fixture.execute(sql).unwrap_or_else(|error| {
            panic!("PostgreSQL-valid custom-range setup failed: {sql}: {error}")
        });
    }

    // Act
    let scan = fixture.execute(
        "SELECT id FROM issue_905_custom_range_scan \
         WHERE value @> CAST(3 AS FLOAT8) ORDER BY id",
    );
    let indexed = fixture.execute(
        "SELECT id FROM issue_905_custom_range_indexed \
         WHERE value @> CAST(3 AS FLOAT8) ORDER BY id",
    );

    // Assert
    let containing = &[vec![Value::Int64(2)]];
    assert_eq!(
        scan.expect("custom range containment scan").rows(),
        containing
    );
    assert_eq!(
        indexed.expect("custom range GiST containment query").rows(),
        containing
    );
}
