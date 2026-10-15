//! PostgreSQL 18 range and multirange value behavior tracked by #905.

use cassie::app::Cassie;
use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[path = "../../types/ranges/multirange_range_merge.rs"]
mod multirange_range_merge;

struct RangeDirectory(String);

impl Drop for RangeDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn should_aggregate_builtin_ranges_into_a_multirange() {
    // Arrange
    let fixture = sql_fixture("issue-905-range-union-aggregate", &[]);
    fixture
        .execute("CREATE TABLE issue_905_range_aggregate_rows (span INT4RANGE)")
        .expect("create a table with range values");
    fixture
        .execute(
            "INSERT INTO issue_905_range_aggregate_rows VALUES ('[1,5)'::INT4RANGE), ('[4,7)'::INT4RANGE), ('[9,11)'::INT4RANGE), (NULL)",
        )
        .expect("insert overlapping, separated and NULL ranges");

    // Act
    let result =
        fixture.execute("SELECT range_agg(span)::TEXT FROM issue_905_range_aggregate_rows");

    // Assert
    assert_eq!(
        result
            .expect("range aggregate across overlapping and separated inputs")
            .rows(),
        &[vec![Value::String("{[1,7),[9,11)}".to_owned())]]
    );
}

#[test]
fn should_intersect_ranges_with_a_range_aggregate() {
    // Arrange
    let fixture = sql_fixture("issue-905-range-intersection-aggregate", &[]);
    fixture
        .execute("CREATE TABLE issue_905_range_intersection_rows (span INT4RANGE)")
        .expect("create a table with range values");
    fixture
        .execute(
            "INSERT INTO issue_905_range_intersection_rows VALUES ('[1,5)'::INT4RANGE), ('[3,7)'::INT4RANGE), ('[4,6)'::INT4RANGE), (NULL)",
        )
        .expect("insert intersecting and NULL ranges");

    // Act
    let result = fixture
        .execute("SELECT range_intersect_agg(span)::TEXT FROM issue_905_range_intersection_rows");

    // Assert
    assert_eq!(
        result
            .expect("range intersection across multiple inputs")
            .rows(),
        &[vec![Value::String("[4,5)".to_owned())]]
    );
}

#[test]
fn should_aggregate_multirange_inputs_into_multiranges() {
    // Arrange
    let fixture = sql_fixture("issue-905-multirange-aggregate-inputs", &[]);
    fixture
        .execute("CREATE TABLE issue_905_multirange_aggregate_inputs (span INT4MULTIRANGE)")
        .expect("create a table with multirange values");
    fixture
        .execute(
            "INSERT INTO issue_905_multirange_aggregate_inputs VALUES ('{[1,5),[9,11)}'::INT4MULTIRANGE), ('{[3,7),[9,12)}'::INT4MULTIRANGE)",
        )
        .expect("insert overlapping and separated multiranges");

    // Act
    let result = fixture.execute(
        "SELECT range_agg(span)::TEXT, range_intersect_agg(span)::TEXT FROM issue_905_multirange_aggregate_inputs",
    );

    // Assert
    assert_eq!(
        result
            .expect("multirange aggregate union and intersection")
            .rows(),
        &[vec![
            Value::String("{[1,7),[9,12)}".to_owned()),
            Value::String("{[3,5),[9,11)}".to_owned()),
        ]]
    );
}

#[test]
fn should_preserve_range_aggregate_empty_nulls() {
    // Arrange
    let fixture = sql_fixture("issue-905-range-aggregate-empty-null", &[]);
    fixture
        .execute("CREATE TABLE issue_905_range_aggregate_edges (kind TEXT, span INT4RANGE)")
        .expect("create range aggregate edge table");
    fixture
        .execute("INSERT INTO issue_905_range_aggregate_edges VALUES ('empty_only', 'empty'::INT4RANGE), ('mixed', 'empty'::INT4RANGE), ('mixed', '[1,3)'::INT4RANGE), ('null_only', NULL)")
        .expect("insert empty, finite, and null ranges");

    // Act
    let result = fixture.execute(
        "SELECT kind, range_agg(span)::TEXT, range_intersect_agg(span)::TEXT FROM issue_905_range_aggregate_edges GROUP BY kind ORDER BY kind",
    );

    // Assert
    assert_eq!(
        result
            .expect("range aggregates preserve empty and null identities")
            .rows(),
        &[
            vec![
                Value::String("empty_only".to_owned()),
                Value::String("{}".to_owned()),
                Value::String("empty".to_owned()),
            ],
            vec![
                Value::String("mixed".to_owned()),
                Value::String("{[1,3)}".to_owned()),
                Value::String("empty".to_owned()),
            ],
            vec![
                Value::String("null_only".to_owned()),
                Value::Null,
                Value::Null
            ],
        ]
    );
}

include!("../../types/ranges/ranges.rs");
