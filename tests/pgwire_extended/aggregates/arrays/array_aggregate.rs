//! PostgreSQL 18 collection aggregate array-input behavior tracked by #811.

use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;
use cassie::types::Value;

fn array_aggregate_setup() -> [(&'static str, Vec<Value>); 2] {
    [
        (
            "CREATE TABLE issue_811_array_aggregate_input (ordinal INT4 PRIMARY KEY, item INT4[])",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_811_array_aggregate_input VALUES (1, $$[0:1]={1,2}$$::INT4[]), (2, $$[0:1]={3,4}$$::INT4[])",
            Vec::new(),
        ),
    ]
}

fn int4_array_element(value: i32, output: &mut Vec<u8>) {
    output.extend_from_slice(&4_i32.to_be_bytes());
    output.extend_from_slice(&value.to_be_bytes());
}

fn nested_array_binary_value() -> Vec<u8> {
    let mut value = Vec::new();
    value.extend_from_slice(&2_i32.to_be_bytes()); // dimensions
    value.extend_from_slice(&0_i32.to_be_bytes()); // no NULL elements
    value.extend_from_slice(&23_i32.to_be_bytes()); // INT4 element OID
    value.extend_from_slice(&2_i32.to_be_bytes()); // outer length
    value.extend_from_slice(&1_i32.to_be_bytes()); // outer lower bound
    value.extend_from_slice(&2_i32.to_be_bytes()); // inner length
    value.extend_from_slice(&0_i32.to_be_bytes()); // preserved inner lower bound
    for element in [1, 2, 3, 4] {
        int4_array_element(element, &mut value);
    }
    value
}

#[test]
fn should_aggregate_array_inputs_into_a_higher_dimension_in_text_wire() {
    // Arrange
    let sql = "SELECT array_agg(item ORDER BY ordinal) FROM issue_811_array_aggregate_input";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles(
        "issue-811-array-aggregate-higher-dimension",
        &array_aggregate_setup(),
        vec![cycle],
    );

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (1_007, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("[1:2][0:1]={{1,2},{3,4}}".to_owned())]]
    );
}

#[test]
fn should_aggregate_array_inputs_into_a_higher_dimension_in_binary_wire() {
    // Arrange
    let sql = "SELECT array_agg(item ORDER BY ordinal) FROM issue_811_array_aggregate_input";
    let cycle = fixture::execute_cycle(sql, None, 1, true);

    // Act
    let batches = fixture::run_cycles(
        "issue-811-array-aggregate-higher-dimension-binary",
        &array_aggregate_setup(),
        vec![cycle],
    );

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (1_007, -1, -1), 1, true);
    assert_eq!(
        fixture::data_row_payloads(frames),
        vec![fixture::one_field_row(&nested_array_binary_value())]
    );
}

#[test]
fn should_reject_typed_null_array_inputs_before_distinct_aggregation() {
    // Arrange
    let cases = [
        (
            "issue-811-typed-null-array-ordinary",
            "SELECT array_agg($1) FROM (VALUES (1),(2)) AS t(n)",
            Some("22004"),
        ),
        (
            "issue-811-typed-null-array-explain-analyze",
            "EXPLAIN ANALYZE SELECT array_agg($1) FROM (VALUES (1),(2)) AS t(n)",
            Some("22004"),
        ),
        (
            "issue-811-typed-null-array-distinct",
            "SELECT array_agg(DISTINCT $1) FROM (VALUES (1),(2)) AS t(n)",
            Some("22004"),
        ),
        (
            "issue-811-typed-null-array-distinct-order",
            "SELECT array_agg(DISTINCT $1 ORDER BY $1) FROM (VALUES (1),(2)) AS t(n)",
            Some("22004"),
        ),
        (
            "issue-811-typed-null-array-distinct-filter-selected",
            "SELECT array_agg(DISTINCT $1) FILTER (WHERE TRUE) FROM (VALUES (1),(2)) AS t(n)",
            Some("22004"),
        ),
        (
            "issue-811-typed-null-array-distinct-filtered-out",
            "SELECT array_agg(DISTINCT $1) FILTER (WHERE FALSE) FROM (VALUES (1),(2)) AS t(n)",
            None,
        ),
    ];

    // Act
    let observations = cases.map(|(label, sql, _)| {
        let cycle = fixture::execute_cycle(sql, Some((1_007, 0, None)), 0, true);
        fixture::run_cycles(label, &[], vec![cycle])
    });

    // Assert
    for ((_, _, sqlstate), batches) in cases.into_iter().zip(observations) {
        let frames = &batches[0];
        if let Some(sqlstate) = sqlstate {
            crate::support_collection_aggregate_resources::assert_rejected_without_partial_rows(
                frames, sqlstate,
            );
        } else {
            fixture::assert_success(frames);
            fixture::assert_single_column_descriptors(frames, (1_007, -1, -1), 0, true);
            assert_eq!(crate::support_pgwire::data_rows(frames), vec![vec![None]]);
        }
    }
}

#[test]
fn should_validate_distinct_array_inputs_with_different_lower_bounds() {
    // Arrange
    let sql = "SELECT array_agg(DISTINCT value) \
               FROM (VALUES (ARRAY[1,1]), (array_fill(1, ARRAY[2], ARRAY[0]))) AS inputs(value)";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles(
        "issue-811-distinct-array-shape-validation",
        &[],
        vec![cycle],
    );

    // Assert
    crate::support_collection_aggregate_resources::assert_rejected_without_partial_rows(
        &batches[0], "2202E",
    );
}
