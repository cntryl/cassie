//! Declared scalar-array text input and unlimited VARCHAR contracts.

use super::support_pgwire as wire;
use super::support_type_metadata_contract as fixture;
use cassie::types::Value;

#[path = "../array_function/binary.rs"]
mod array_function_gaps;
#[path = "../array_function/errors.rs"]
mod array_function_residuals;
#[path = "../array_position_start_overloads.rs"]
mod array_position_start_overloads;
#[path = "../array_randomization_custom_bounds.rs"]
mod array_randomization_custom_bounds;
#[path = "../array_reverse_custom_bounds.rs"]
mod array_reverse_custom_bounds;
#[path = "../array_sort_custom_bounds.rs"]
mod array_sort_custom_bounds;
#[path = "../binary_container_framing.rs"]
mod binary_container_framing;
#[path = "../boolean_array_container_framing.rs"]
mod boolean_array_container_framing;
#[path = "../multidimensional_persistence_binary.rs"]
mod multidimensional_persistence_binary;
#[path = "../multidimensional_slices.rs"]
mod multidimensional_slices;
#[path = "../ragged_array_constructors.rs"]
mod ragged_array_constructors;
#[path = "../standard_oid_dml_null_identity.rs"]
mod standard_oid_dml_null_identity;
#[path = "../three_dimensional_bounds_restart.rs"]
mod three_dimensional_bounds_restart;

type IntegerArrayEndpoint = (
    i32,
    &'static [u8],
    &'static [u8],
    &'static [u8],
    &'static [u8],
);

fn unlimited_varchar_reads() -> Vec<fixture::Cycle> {
    [0, 1]
        .into_iter()
        .flat_map(|format| {
            [
                fixture::execute_cycle(
                    "SELECT v FROM finite_unlimited_varchar WHERE k <= 3 ORDER BY k",
                    None,
                    format,
                    true,
                ),
                fixture::execute_cycle(
                    "SELECT v FROM finite_unlimited_varchar_array WHERE k <= 3 ORDER BY k",
                    None,
                    format,
                    true,
                ),
            ]
        })
        .collect()
}

#[test]
fn should_aggregate_integer_rows_into_a_postgresql_int4_array() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_810_array_aggregate_values (value INTEGER)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_810_array_aggregate_values VALUES (2)",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT ARRAY_AGG(value) FROM issue_810_array_aggregate_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-810-array-aggregate-values", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (1007, -1, -1), 0, true);
    assert_eq!(wire::data_rows(frames), vec![vec![Some("{2}".to_owned())]]);
}

#[test]
fn should_order_values_in_postgresql_int4_array_aggregate() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_810_array_aggregate_input (value INTEGER)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_810_array_aggregate_input VALUES (2), (1), (3)",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT ARRAY_AGG(value ORDER BY value) FROM issue_810_array_aggregate_input",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-810-ordered-array-aggregate", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (1007, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("{1,2,3}".to_owned())]]
    );
}

#[test]
fn should_aggregate_array_values_into_a_higher_dimension_array() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_811_array_aggregate_values (id INT, items INT[])",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_811_array_aggregate_values VALUES (1, $1), (2, $2)",
            vec![
                Value::Json(serde_json::json!([1, 2])),
                Value::Json(serde_json::json!([3, 4])),
            ],
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT array_agg(items) FROM issue_811_array_aggregate_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles(
        "issue-811-array-aggregate-array-inputs",
        &setup,
        vec![cycle],
    );

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (1007, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("{{1,2},{3,4}}".to_owned())]]
    );
}

#[test]
fn should_distinct_array_aggregate_inputs_before_ordering() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_811_distinct_array_inputs (value INT)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_811_distinct_array_inputs VALUES (2), (1), (2), (3)",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT array_agg(DISTINCT value ORDER BY value DESC) FROM issue_811_distinct_array_inputs",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-811-distinct-array-aggregate", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (1_007, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("{3,2,1}".to_owned())]]
    );
}

include!("../bounds_subscripts/nulls.rs");

include!("../bounds_subscripts/semantics.rs");

include!("../dml_identity/semantics.rs");

include!("../functions/nulls.rs");

include!("../index_queries/restart.rs");

include!("../index_queries/semantics.rs");

include!("../operators/nulls.rs");

include!("../operators/parameters.rs");

include!("../operators/semantics.rs");

include!("../text_inputs/binary.rs");

include!("../text_inputs/errors.rs");

include!("../text_inputs/nulls.rs");

include!("../text_inputs/parameters.rs");

include!("../text_inputs/resources.rs");

include!("../text_inputs/restart.rs");

include!("../text_inputs/semantics.rs");
