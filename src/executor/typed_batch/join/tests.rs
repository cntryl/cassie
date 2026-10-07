use super::HashJoin;
use crate::executor::typed_batch::TypedBatch;
use crate::runtime::QueryExecutionControls;
use crate::types::{DataType, Value};

#[test]
fn should_preserve_typed_join_duplicates_nulls_and_payload_in_capped_batches() {
    // Arrange
    let limits = crate::config::CassieRuntimeLimits::default();
    let controls = QueryExecutionControls::from_limits(&limits, std::time::Instant::now());
    let schema = [
        ("key".to_owned(), DataType::BigInt),
        ("payload".to_owned(), DataType::Text),
    ];
    let left = TypedBatch::from_columns(
        &controls,
        &schema,
        &[
            vec![Value::Int64(1), Value::Null, Value::Int64(2)],
            vec![
                Value::String("left".into()),
                Value::String("null".into()),
                Value::String("unmatched".into()),
            ],
        ],
        3,
        Some(&[0, 0, 1, 2]),
    )
    .expect("left input");
    let right = TypedBatch::from_columns(
        &controls,
        &schema,
        &[
            vec![Value::Int64(1), Value::Int64(1), Value::Null],
            vec![
                Value::String("first".into()),
                Value::String("second".into()),
                Value::String("never".into()),
            ],
        ],
        3,
        None,
    )
    .expect("right input");
    let mut join =
        HashJoin::new(&controls, &left, &right, (0, 0), true, 2, |_| Ok(())).expect("typed join");

    // Act
    let mut values = Vec::new();
    let mut lengths = Vec::new();
    while let Some(output) = join.next_batch(usize::MAX).expect("joined batch") {
        let batch = output.batch;
        lengths.push(batch.len());
        assert_eq!(batch.schema().len(), 4);
        for lane in 0..batch.len() {
            values.push((batch.value(1, lane).unwrap(), batch.value(3, lane).unwrap()));
        }
    }

    // Assert
    assert_eq!(lengths, vec![2, 2, 2]);
    assert_eq!(
        values,
        vec![
            (Value::String("left".into()), Value::String("first".into())),
            (Value::String("left".into()), Value::String("second".into())),
            (Value::String("left".into()), Value::String("first".into())),
            (Value::String("left".into()), Value::String("second".into())),
            (Value::String("null".into()), Value::Null),
            (Value::String("unmatched".into()), Value::Null),
        ]
    );
}

#[test]
fn should_match_exact_mixed_numeric_and_signed_zero_keys() {
    // Arrange
    let controls = controls();
    let left = batch(
        &controls,
        DataType::BigInt,
        vec![
            Value::Int64(0),
            Value::Int64(9_007_199_254_740_993),
            Value::Int64(9_007_199_254_740_992),
        ],
    );
    let right = batch(
        &controls,
        DataType::Float,
        vec![
            Value::Float64(-0.0),
            Value::Float64(0.0),
            Value::Float64(9_007_199_254_740_992.0),
        ],
    );
    let mut join = HashJoin::new(&controls, &left, &right, (0, 0), false, 1, |_| Ok(())).unwrap();

    // Act
    let mut rows = Vec::new();
    while let Some(output) = join.next_batch(usize::MAX).unwrap() {
        rows.push((
            output.batch.value(0, 0).unwrap(),
            output.batch.value(1, 0).unwrap(),
        ));
    }

    // Assert
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].0, Value::Int64(0));
    assert_eq!(rows[1].0, Value::Int64(0));
    assert_eq!(rows[2].0, Value::Int64(9_007_199_254_740_992));
    assert!(matches!(rows[0].1, Value::Float64(value) if value.to_bits() == (-0.0_f64).to_bits()));
}

#[test]
fn should_extend_empty_typed_right_input_with_complete_outer_schema() {
    // Arrange
    let controls = controls();
    let left = batch(
        &controls,
        DataType::Boolean,
        vec![Value::Bool(true), Value::Null],
    );
    let right = TypedBatch::from_columns(
        &controls,
        &[
            ("key".into(), DataType::Boolean),
            ("text".into(), DataType::Text),
        ],
        &[vec![], vec![]],
        0,
        None,
    )
    .unwrap();
    let mut join = HashJoin::new(&controls, &left, &right, (0, 0), true, 1, |_| Ok(())).unwrap();

    // Act
    let mut count = 0;
    while let Some(output) = join.next_batch(usize::MAX).unwrap() {
        assert_eq!(output.batch.schema().len(), 3);
        assert_eq!(output.batch.schema()[2].1, DataType::Text);
        assert_eq!(output.batch.value(1, 0).unwrap(), Value::Null);
        assert_eq!(output.batch.value(2, 0).unwrap(), Value::Null);
        count += output.batch.len();
    }

    // Assert
    assert_eq!(count, 2);
    let empty = batch(&controls, DataType::Boolean, vec![]);
    let mut empty_join =
        HashJoin::new(&controls, &empty, &right, (0, 0), false, 1, |_| Ok(())).unwrap();
    assert!(empty_join.next_batch(usize::MAX).unwrap().is_none());
}

#[test]
fn should_admit_hash_chain_before_demanding_any_build_lane() {
    // Arrange
    let controls = controls();
    let left = batch(&controls, DataType::BigInt, vec![Value::Int64(1)]);
    let right = batch(&controls, DataType::BigInt, vec![Value::Int64(1); 1024]);
    let occupied = controls
        .reserve_query_memory(
            controls.query_memory_budget_bytes - controls.current_query_memory_bytes(),
        )
        .unwrap();
    let called = std::cell::Cell::new(false);

    // Act
    let result = HashJoin::new(&controls, &left, &right, (0, 0), false, 2, |_| {
        called.set(true);
        Ok(())
    });

    // Assert
    assert!(matches!(
        result.map_err(crate::app::CassieError::from),
        Err(crate::app::CassieError::ResourceLimit(_))
    ));
    assert!(!called.get());
    drop(occupied);
}

#[test]
fn should_release_native_hash_state_when_build_is_cancelled() {
    // Arrange
    let limits = crate::config::CassieRuntimeLimits::default();
    let cancellation = crate::runtime::QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &limits,
        std::time::Instant::now(),
        cancellation.clone(),
    );
    let left = batch(&controls, DataType::BigInt, vec![Value::Int64(1)]);
    let right = batch(&controls, DataType::BigInt, vec![Value::Int64(1); 16]);
    let before = controls.current_query_memory_bytes();

    // Act
    let result = HashJoin::new(&controls, &left, &right, (0, 0), false, 2, |lane| {
        if lane == 1 {
            cancellation.cancel();
        }
        Ok(())
    });

    // Assert
    assert!(matches!(
        result.map_err(crate::app::CassieError::from),
        Err(crate::app::CassieError::QueryCancelled)
    ));
    assert_eq!(controls.current_query_memory_bytes(), before);
}

#[test]
fn should_release_match_maps_when_hot_key_expansion_is_cancelled() {
    // Arrange
    let limits = crate::config::CassieRuntimeLimits::default();
    let cancellation = crate::runtime::QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &limits,
        std::time::Instant::now(),
        cancellation.clone(),
    );
    let left = batch(&controls, DataType::BigInt, vec![Value::Int64(1)]);
    let right = batch(&controls, DataType::BigInt, vec![Value::Int64(1); 1024]);
    let mut join = HashJoin::new(&controls, &left, &right, (0, 0), false, 64, |_| Ok(())).unwrap();
    let before = controls.current_query_memory_bytes();
    let ticks = std::cell::Cell::new(0);

    // Act
    let result = join.next_batch_with_probe(usize::MAX, || {
        ticks.set(ticks.get() + 1);
        if ticks.get() == 3 {
            cancellation.cancel();
        }
    });

    // Assert
    assert!(matches!(
        result.map_err(crate::app::CassieError::from),
        Err(crate::app::CassieError::QueryCancelled)
    ));
    assert_eq!(ticks.get(), 3);
    assert_eq!(controls.current_query_memory_bytes(), before);
    drop(join);
    drop(left);
    drop(right);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

fn controls() -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &crate::config::CassieRuntimeLimits::default(),
        std::time::Instant::now(),
    )
}

fn batch(controls: &QueryExecutionControls, data_type: DataType, values: Vec<Value>) -> TypedBatch {
    let domain = values.len();
    TypedBatch::from_columns(
        controls,
        &[("key".into(), data_type)],
        &[values],
        domain,
        None,
    )
    .unwrap()
}

#[path = "../tests/families.rs"]
mod payload_fixture;

#[test]
fn should_keep_every_typed_payload_family_alive_after_join_inputs_drop() {
    // Arrange
    let controls = controls();
    let families = payload_fixture::logical_families();
    let mut schema = vec![("key".to_owned(), DataType::BigInt)];
    schema.extend(
        families
            .iter()
            .enumerate()
            .map(|(index, (data_type, _))| (format!("payload_{index}"), data_type.clone())),
    );
    let mut values = vec![vec![Value::Int64(1); 3]];
    values.extend(families.iter().map(|(_, values)| values.clone()));
    let left = TypedBatch::from_columns(&controls, &schema, &values, 3, None).unwrap();
    let right = batch(&controls, DataType::BigInt, vec![Value::Int64(1)]);
    let mut join = HashJoin::new(&controls, &left, &right, (0, 0), false, 3, |_| Ok(())).unwrap();

    // Act
    let output = join.next_batch(usize::MAX).unwrap().unwrap();
    drop(join);
    drop(left);
    drop(right);

    // Assert
    assert!(controls.current_query_memory_bytes() > 0);
    for (index, (data_type, expected)) in families.iter().enumerate() {
        assert_eq!(&output.batch.schema()[index + 1].1, data_type);
        for (lane, expected) in expected.iter().enumerate() {
            let actual = output.batch.value(index + 1, lane).unwrap();
            match (&actual, expected) {
                (Value::Float64(actual), Value::Float64(expected)) => {
                    assert_eq!(actual.to_bits(), expected.to_bits())
                }
                _ => assert_eq!(&actual, expected),
            }
        }
    }
    drop(output);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
