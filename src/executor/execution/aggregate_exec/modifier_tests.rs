//! Values and ownership of the actual aggregate modifier fallback.

use super::*;
use crate::types::DataType;
use std::sync::Arc;

fn call(plan: &LogicalPlan) -> &FunctionCall {
    let SelectItem::Function { function, .. } = &plan.projection[0] else {
        panic!("aggregate projection");
    };
    function
}

#[test]
fn should_skip_argument_evaluation_given_false_and_null_filters_when_aggregate_transitions_execute()
{
    // Arrange
    let plan = aggregate_plan("SELECT COUNT(8 / denominator) FILTER (WHERE included) FROM events");
    let functions = HashMap::new();
    let controls = controls_with_budget(1_048_576);
    let context = aggregate_context(&plan, &functions, &controls);
    let rows = [
        (0, Value::Bool(false)),
        (0, Value::Null),
        (2, Value::Bool(true)),
    ]
    .map(|(denominator, included)| {
        BatchRow::new(vec![
            ("denominator".into(), Value::Int64(denominator)),
            ("included".into(), included),
        ])
    });

    // Act
    let result = AggregateAccumulator::evaluate(call(&plan), &rows, &context);

    // Assert
    assert_eq!(
        result.expect("excluded zero denominators are never evaluated"),
        Value::Int64(1)
    );
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_deduplicate_globally_given_repeated_values_across_partitions_when_distinct_selects_a_serial_fallback(
) {
    // Arrange
    let plan = aggregate_plan("SELECT COUNT(DISTINCT payload) FROM events");
    let functions = HashMap::new();
    let controls = controls_with_budget(1_048_576);
    let context = aggregate_context(&plan, &functions, &controls);
    let rows = (0..1_025)
        .map(|index| BatchRow::new(vec![("payload".into(), Value::Int64(i64::from(index % 3)))]))
        .collect::<Vec<_>>();

    // Act
    let eligibility = parallel_aggregation_eligibility(&plan, &aggregate_specs(&plan), &functions);
    let result = AggregateAccumulator::evaluate(call(&plan), &rows, &context);

    // Assert
    assert_eq!(eligibility, Err("aggregate-local-distinct-or-order"));
    assert_eq!(result.expect("one global DISTINCT owner"), Value::Int64(3));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_bound_distinct_and_ordered_extrema_given_wide_text_typed_arrays_and_nulls_when_admission_is_exact(
) {
    // Arrange
    let plan = aggregate_plan(
        "SELECT MIN(DISTINCT payload ORDER BY payload DESC NULLS FIRST) FROM events",
    );
    let functions = HashMap::new();
    let cases = [
        (
            DataType::Text,
            Value::String("z".repeat(8_192)),
            Value::String("a".repeat(8_192)),
        ),
        (
            DataType::Array(Box::new(DataType::Int)),
            Value::Json(serde_json::json!([10, 1])),
            Value::Json(serde_json::json!([2, 1])),
        ),
        (
            DataType::Array(Box::new(DataType::Text)),
            Value::Json(serde_json::json!(["z".repeat(8_192)])),
            Value::Json(serde_json::json!(["a".repeat(8_192)])),
        ),
    ];

    // Act
    for (data_type, high, low) in cases {
        let types = Arc::new(vec![data_type]);
        let rows = [Value::Null, high.clone(), low.clone(), high].map(|value| {
            let mut row = BatchRow::new(vec![("payload".into(), value)]);
            row.set_data_types(types.clone());
            row
        });
        let ample = controls_with_budget(1_048_576);
        let measured = AggregateAccumulator::evaluate(
            call(&plan),
            &rows,
            &aggregate_context(&plan, &functions, &ample),
        );
        let peak = ample.peak_query_memory_bytes();
        let exact = controls_with_budget(peak);
        let denied = controls_with_budget(peak - 1);
        let admitted = AggregateAccumulator::evaluate(
            call(&plan),
            &rows,
            &aggregate_context(&plan, &functions, &exact),
        );
        let failure = AggregateAccumulator::evaluate(
            call(&plan),
            &rows,
            &aggregate_context(&plan, &functions, &denied),
        );

        // Assert
        assert_eq!(measured.expect("measure distinct ordered extrema"), low);
        assert_eq!(admitted.expect("exact admission succeeds"), low);
        assert!(failure
            .expect_err("one byte below peak must fail")
            .to_string()
            .contains("query memory budget exceeded"));
        assert_eq!(exact.peak_query_memory_bytes(), peak);
        for (tracker, budget) in [(&ample, 1_048_576), (&exact, peak), (&denied, peak - 1)] {
            assert_eq!(tracker.current_query_memory_bytes(), 0);
            let reused = tracker
                .reserve_query_memory(budget)
                .expect("whole owner budget is reusable");
            drop(reused);
        }
    }
}

#[test]
fn should_release_retained_modifier_owners_given_cancellation_when_distinct_and_ordered_inputs_have_grown(
) {
    // Arrange
    let plan = aggregate_plan("SELECT MIN(DISTINCT payload ORDER BY payload) FROM events");
    let functions = HashMap::new();
    let cancellation = crate::runtime::QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let mut context = aggregate_context(&plan, &functions, &controls);
    let observer = |retained| {
        assert_eq!(retained, 1);
        assert!(controls.current_query_memory_bytes() >= 8_192);
        cancellation.cancel();
    };
    context.after_partition_row = Some(&observer);
    let rows = ["z", "a"]
        .map(|value| BatchRow::new(vec![("payload".into(), Value::String(value.repeat(8_192)))]));

    // Act
    let result = AggregateAccumulator::evaluate(call(&plan), &rows, &context);

    // Assert
    assert!(result
        .expect_err("cancelled retained aggregate cannot finish")
        .to_string()
        .contains("query canceled"));
    assert_eq!(controls.current_query_memory_bytes(), 0);
    let reused = controls
        .reserve_query_memory(CassieRuntimeLimits::default().query_memory_budget_bytes)
        .expect("cancelled owner releases memory");
    drop(reused);
}

#[test]
fn should_admit_non_input_order_keys_given_wide_text_functions_when_expression_temporaries_are_created(
) {
    // Arrange
    let functions = HashMap::new();
    let rows = ["z", "a"].map(|text| {
        BatchRow::new(vec![
            ("amount".into(), Value::Int64(1)),
            ("payload".into(), Value::String(text.repeat(8_192))),
        ])
    });
    let cases = [
        (
            "SELECT SUM(amount ORDER BY UPPER(payload)) FROM events",
            Value::Int64(2),
        ),
        (
            "SELECT COUNT(UPPER(payload) ORDER BY UPPER(payload)) FROM events",
            Value::Int64(2),
        ),
    ];

    // Act
    for (sql, expected) in cases {
        let plan = aggregate_plan(sql);
        let ample = controls_with_budget(1_048_576);
        let measured = AggregateAccumulator::evaluate(
            call(&plan),
            &rows,
            &aggregate_context(&plan, &functions, &ample),
        );
        let peak = ample.peak_query_memory_bytes();
        let exact = controls_with_budget(peak);
        let denied = controls_with_budget(peak - 1);
        let admitted = AggregateAccumulator::evaluate(
            call(&plan),
            &rows,
            &aggregate_context(&plan, &functions, &exact),
        );
        let failure = AggregateAccumulator::evaluate(
            call(&plan),
            &rows,
            &aggregate_context(&plan, &functions, &denied),
        );

        // Assert
        assert_eq!(
            measured.expect("measure ordered expression admission"),
            expected
        );
        assert_eq!(admitted.expect("exact admission succeeds"), expected);
        assert!(failure
            .expect_err("temporary expression growth is admitted")
            .to_string()
            .contains("query memory budget exceeded"));
        assert!(peak >= 2 * 8_192);
        for tracker in [&ample, &exact, &denied] {
            assert_eq!(tracker.current_query_memory_bytes(), 0);
        }
    }
}
