//! NULL and worker-boundary witnesses for the actual Boolean typed states.

use super::*;
use crate::config::CassieRuntimeLimits;
use std::time::Instant;

#[test]
fn should_merge_boolean_typed_states_given_empty_null_and_mixed_partials_when_every_boundary_is_used(
) {
    // Arrange
    let cases = [
        (vec![], Value::Null, Value::Null),
        (vec![Value::Null, Value::Null], Value::Null, Value::Null),
        (
            vec![Value::Bool(true), Value::Null, Value::Bool(true)],
            Value::Bool(true),
            Value::Bool(true),
        ),
        (
            vec![Value::Bool(false), Value::Null, Value::Bool(false)],
            Value::Bool(false),
            Value::Bool(false),
        ),
        (
            vec![
                Value::Null,
                Value::Bool(true),
                Value::Bool(false),
                Value::Null,
            ],
            Value::Bool(false),
            Value::Bool(true),
        ),
    ];
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());

    // Act
    for (values, and, or) in cases {
        for name in ["bool_and", "bool_or", "every", "pg_catalog.every"] {
            for split in 0..=values.len() {
                let mut state = State::new(name).expect("Boolean state");
                for values in [&values[..split], &values[split..], &[]] {
                    let batch = TypedBatch::from_columns(
                        &controls,
                        &[("flag".to_owned(), DataType::Boolean)],
                        &[values.to_vec()],
                        values.len(),
                        None,
                    )
                    .expect("native Boolean lanes");
                    let mut partial = state.empty_partial();
                    consume(&mut partial, Some(0), &batch, &controls)
                        .expect("fold this Boolean partition");
                    state.merge_ordered(&partial).expect("merge partial");
                }

                // Assert
                let expected = if name == "bool_or" { &or } else { &and };
                assert_eq!(&state.finish().expect("finish"), expected, "{name}/{split}");
                assert_eq!(controls.current_query_memory_bytes(), 0);
            }
        }
    }
}

#[test]
fn should_release_boolean_typed_owners_given_cancellation_when_a_partition_is_consumed() {
    // Arrange
    let cancellation = crate::runtime::QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let batch = TypedBatch::from_columns(
        &controls,
        &[("flag".to_owned(), DataType::Boolean)],
        &[vec![Value::Bool(false), Value::Bool(true)]],
        2,
        None,
    )
    .expect("Boolean source");
    let mut state = State::new("bool_and").expect("Boolean state");
    cancellation.cancel();

    // Act
    let result = consume(&mut state, Some(0), &batch, &controls);
    drop(batch);

    // Assert
    assert!(
        result.is_err(),
        "cancelled input cannot produce a completed aggregate"
    );
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_mismatched_boolean_partials_given_different_aggregate_kinds_when_merging() {
    // Arrange
    let mut and = State::new("bool_and").expect("AND state");
    let mut or = State::new("bool_or").expect("OR state");
    and.update(Cell::Boolean(false)).expect("false input");
    or.update(Cell::Boolean(true)).expect("true input");

    // Act
    let result = and.merge_ordered(&or);

    // Assert
    assert!(
        result.is_err(),
        "partial states cannot change the aggregate operation"
    );
    assert_eq!(
        and.finish().expect("AND remains unchanged"),
        Value::Bool(false)
    );
}
