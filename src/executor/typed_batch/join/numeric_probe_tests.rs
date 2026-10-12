use std::cell::Cell;
use std::time::Instant;

use crate::app::CassieError;
use crate::config::CassieRuntimeLimits;
use crate::executor::typed_batch::join::HashJoin;
use crate::executor::typed_batch::TypedBatch;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use crate::sql::numeric_comparison::NumericComparisonDomain;
use crate::types::{DataType, Value};

#[derive(Clone, Copy)]
enum Mode {
    Output,
    Admission,
    Cancel,
}

struct Observation {
    input_bytes: usize,
    build_bytes: usize,
    calls: usize,
    denied: bool,
    cancelled: bool,
    rows: Vec<(Value, Value)>,
    after_kernel: usize,
    retired: usize,
}

fn observe(budget: usize, reverse: bool, mode: Mode) -> Observation {
    let limits = CassieRuntimeLimits {
        query_timeout_ms: 0,
        query_memory_budget_bytes: budget,
        ..CassieRuntimeLimits::default()
    };
    let cancellation = QueryCancellationHandle::new();
    let controls =
        QueryExecutionControls::with_cancellation(&limits, Instant::now(), cancellation.clone());
    let integer = TypedBatch::from_columns(
        &controls,
        &[("integer_key".into(), DataType::BigInt)],
        &[vec![
            Value::Int64(9_007_199_254_740_993),
            Value::Int64(9_007_199_254_740_992),
            Value::Null,
        ]],
        3,
        None,
    )
    .expect("integer typed input");
    let float = TypedBatch::from_columns(
        &controls,
        &[("float_key".into(), DataType::Float)],
        &[vec![
            Value::Float64(9_007_199_254_740_992.0),
            Value::Float64(9_007_199_254_740_992.0),
            Value::Null,
        ]],
        3,
        None,
    )
    .expect("float typed input");
    let input_bytes = controls.current_query_memory_bytes();
    let (left, right) = if reverse {
        (&float, &integer)
    } else {
        (&integer, &float)
    };
    let calls = Cell::new(0);
    let build_bytes = Cell::new(0);
    let result = HashJoin::new(
        &controls,
        left,
        right,
        (0, 0, NumericComparisonDomain::Float8),
        false,
        2,
        |lane| {
            calls.set(calls.get() + 1);
            build_bytes.set(controls.current_query_memory_bytes() - input_bytes);
            if matches!(mode, Mode::Cancel) && lane == 1 {
                cancellation.cancel();
            }
            Ok(())
        },
    )
    .map_err(CassieError::from);
    let mut rows = Vec::new();
    let mut denied = false;
    let mut cancelled = false;
    match result {
        Ok(mut kernel) => {
            if matches!(mode, Mode::Output) {
                while let Some(output) = kernel.next_batch(64).expect("mixed typed output") {
                    for lane in 0..output.batch.len() {
                        let first = output.batch.value(0, lane).expect("left carrier");
                        let second = output.batch.value(1, lane).expect("right carrier");
                        rows.push(if reverse {
                            (second, first)
                        } else {
                            (first, second)
                        });
                    }
                }
            }
        }
        Err(CassieError::ResourceLimit(_)) => denied = true,
        Err(CassieError::QueryCancelled) => cancelled = true,
        Err(error) => panic!("unexpected typed join failure: {error}"),
    }
    let after_kernel = controls.current_query_memory_bytes();
    drop(integer);
    drop(float);
    Observation {
        input_bytes,
        build_bytes: build_bytes.get(),
        calls: calls.get(),
        denied,
        cancelled,
        rows,
        after_kernel,
        retired: controls.current_query_memory_bytes(),
    }
}

#[test]
fn should_bound_mixed_typed_hash_ownership_in_both_build_orientations() {
    // Arrange
    let expected = vec![
        (
            Value::Int64(9_007_199_254_740_993),
            Value::Float64(9_007_199_254_740_992.0),
        ),
        (
            Value::Int64(9_007_199_254_740_993),
            Value::Float64(9_007_199_254_740_992.0),
        ),
        (
            Value::Int64(9_007_199_254_740_992),
            Value::Float64(9_007_199_254_740_992.0),
        ),
        (
            Value::Int64(9_007_199_254_740_992),
            Value::Float64(9_007_199_254_740_992.0),
        ),
    ];

    // Act
    let observations = [false, true].map(|reverse| {
        let ample = observe(1024 * 1024, reverse, Mode::Output);
        let boundary = ample.input_bytes + ample.build_bytes;
        let admitted = observe(boundary, reverse, Mode::Admission);
        let denied = observe(boundary - 1, reverse, Mode::Admission);
        let cancelled = observe(1024 * 1024, reverse, Mode::Cancel);
        (ample, admitted, denied, cancelled)
    });

    // Assert
    for (mut ample, admitted, denied, cancelled) in observations {
        ample.rows.sort_by_key(|(value, _)| match value {
            Value::Int64(value) => *value,
            _ => panic!("original integer carrier"),
        });
        let mut expected = expected.clone();
        expected.sort_by_key(|(value, _)| match value {
            Value::Int64(value) => *value,
            _ => panic!("literal integer carrier"),
        });
        assert_eq!(
            ample.rows, expected,
            "duplicates match after Float8 normalization; NULL keys do not match"
        );
        assert!(ample.input_bytes > 0 && ample.build_bytes >= 64);
        assert_eq!(
            admitted.calls, 3,
            "each real build lane is reached at exact admission"
        );
        assert!(!admitted.denied && !admitted.cancelled);
        assert_eq!(admitted.build_bytes, ample.build_bytes);
        assert!(denied.denied);
        assert_eq!(
            denied.calls, 0,
            "one byte short rejects before a build key is demanded"
        );
        assert!(cancelled.cancelled);
        assert_eq!(
            cancelled.calls, 2,
            "cancellation is triggered by the second actual build lane"
        );
        for observation in [&ample, &admitted, &denied, &cancelled] {
            assert_eq!(
                observation.after_kernel, observation.input_bytes,
                "failed/completed build releases only its own lease"
            );
            assert_eq!(
                observation.retired, 0,
                "input owner retirement releases the remaining charge"
            );
        }
    }
}
