//! Actual value-first drop and reached aggregate cancellation witnesses.
use super::*;
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryCancellationHandle;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

struct DropProbe {
    controls: QueryExecutionControls,
    observed: Arc<AtomicUsize>,
}

impl Drop for DropProbe {
    fn drop(&mut self) {
        self.observed
            .store(self.controls.current_query_memory_bytes(), Ordering::SeqCst);
    }
}

fn controls() -> QueryExecutionControls {
    QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now())
}

#[test]
fn should_drop_input_values_while_both_reservations_are_still_live() {
    // Arrange
    let controls = controls();
    let observed = Arc::new(AtomicUsize::new(0));
    let input = inputs::RetainedInputs {
        values: vec![DropProbe {
            controls: controls.clone(),
            observed: Arc::clone(&observed),
        }],
        retained: controls.reserve_query_memory(17).expect("payload lease"),
        capacity: controls.reserve_query_memory(23).expect("directory lease"),
    };

    // Act
    drop(input);

    // Assert
    assert_eq!(observed.load(Ordering::SeqCst), 40);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_drop_ordered_keys_before_the_direct_payload_reservation() {
    // Arrange
    let controls = controls();
    let observed = Arc::new(AtomicUsize::new(0));
    let direct = inputs::RetainedInputs::<()> {
        values: Vec::new(),
        retained: controls
            .reserve_query_memory(11)
            .expect("direct payload lease"),
        capacity: controls
            .reserve_query_memory(7)
            .expect("direct directory lease"),
    };
    let ordered = inputs::RetainedInputs {
        values: vec![vec![DropProbe {
            controls: controls.clone(),
            observed: Arc::clone(&observed),
        }]],
        retained: controls
            .reserve_query_memory(13)
            .expect("key directory lease"),
        capacity: controls
            .reserve_query_memory(5)
            .expect("row directory lease"),
    };

    // Act
    drop(ordered);

    // Assert
    assert_eq!(observed.load(Ordering::SeqCst), 36);
    assert_eq!(controls.current_query_memory_bytes(), 18);
    drop(direct);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

fn plan(sql: &str) -> crate::planner::logical::LogicalPlan {
    let catalog = crate::catalog::Catalog::new();
    catalog.register_collection(
        "events",
        vec![("value".to_owned(), crate::types::DataType::Text)],
    );
    let parsed = crate::sql::parse_statement(sql).expect("parse ordered set");
    crate::executor::execution::build_logical_plan(&catalog, &parsed).expect("plan")
}

#[test]
fn should_cancel_mode_after_a_real_input_is_retained() {
    // Arrange
    let plan = plan("SELECT mode() WITHIN GROUP (ORDER BY value) FROM events");
    let specs = super::super::aggregate_specs(&plan);
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let functions = std::collections::HashMap::new();
    let context = AggregateExecutionContext {
        plan: &plan,
        params: &[],
        parameter_type_oids: &[],
        search_context: None,
        user_functions: &functions,
        session: None,
        controls: &controls,
        after_partition_row: None,
    };
    let rows = [BatchRow::new(vec![(
        "value".into(),
        Value::String("tracked".into()),
    )])];
    let mut reached = false;

    // Act
    let result = mode::evaluate_observed(&specs[0].function, &rows, &context, || {
        reached = true;
        assert!(controls.current_query_memory_bytes() > 0);
        cancellation.cancel();
    });

    // Assert
    assert!(reached);
    assert!(matches!(result, Err(QueryError::General(ref message)) if message == "query canceled"));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_cancel_hypothetical_direct_or_ordered_inputs_after_real_retention() {
    // Arrange
    let plan = plan("SELECT rank('middle') WITHIN GROUP (ORDER BY value) FROM events");
    let specs = super::super::aggregate_specs(&plan);
    let functions = std::collections::HashMap::new();
    let rows = [BatchRow::new(vec![(
        "value".into(),
        Value::String("tracked".into()),
    )])];
    for boundary in [
        hypothetical::InputProgress::Direct,
        hypothetical::InputProgress::Ordered,
    ] {
        let cancellation = QueryCancellationHandle::new();
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            cancellation.clone(),
        );
        let context = AggregateExecutionContext {
            plan: &plan,
            params: &[],
            parameter_type_oids: &[],
            search_context: None,
            user_functions: &functions,
            session: None,
            controls: &controls,
            after_partition_row: None,
        };
        let mut reached = false;

        // Act
        let result =
            hypothetical::evaluate_observed(&specs[0].function, &rows, &context, |progress| {
                if progress == boundary {
                    reached = true;
                    assert!(controls.current_query_memory_bytes() > 0);
                    cancellation.cancel();
                }
            });

        // Assert
        assert!(reached);
        assert!(
            matches!(result, Err(QueryError::General(ref message)) if message == "query canceled")
        );
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[derive(Clone, Copy)]
enum RetentionStage {
    Mode,
    Direct,
    Ordered,
}

fn reached_cancellation_case(
    sql: &str,
    values: &[&str],
    stage: RetentionStage,
) -> (Result<Value, QueryError>, bool, usize) {
    let plan = plan(sql);
    let specs = super::super::aggregate_specs(&plan);
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let functions = std::collections::HashMap::new();
    let context = AggregateExecutionContext {
        plan: &plan,
        params: &[],
        parameter_type_oids: &[],
        search_context: None,
        user_functions: &functions,
        session: None,
        controls: &controls,
        after_partition_row: None,
    };
    let rows = values
        .iter()
        .map(|value| BatchRow::new(vec![("value".into(), Value::String((*value).into()))]))
        .collect::<Vec<_>>();
    let mut reached = false;
    let result = match stage {
        RetentionStage::Mode => {
            mode::evaluate_observed(&specs[0].function, &rows, &context, || {
                reached = true;
                assert!(controls.current_query_memory_bytes() > 0);
                cancellation.cancel();
            })
        }
        RetentionStage::Direct | RetentionStage::Ordered => {
            hypothetical::evaluate_observed(&specs[0].function, &rows, &context, |progress| {
                let selected = matches!(
                    (stage, progress),
                    (RetentionStage::Direct, hypothetical::InputProgress::Direct)
                        | (
                            RetentionStage::Ordered,
                            hypothetical::InputProgress::Ordered
                        )
                );
                if selected {
                    reached = true;
                    assert!(controls.current_query_memory_bytes() > 0);
                    cancellation.cancel();
                }
            })
        }
    };
    (result, reached, controls.current_query_memory_bytes())
}

#[test]
fn should_check_cancellation_after_current_retention_before_later_input_errors() {
    // Arrange
    let cases = [
        (RetentionStage::Mode, "SELECT mode() WITHIN GROUP (ORDER BY value::FLOAT8) FROM events", vec!["1", "bad"]),
        (RetentionStage::Direct, "SELECT rank(1::FLOAT8, 'bad'::FLOAT8) WITHIN GROUP (ORDER BY value::FLOAT8, value::FLOAT8) FROM events", vec!["1"]),
        (RetentionStage::Ordered, "SELECT rank(0::FLOAT8) WITHIN GROUP (ORDER BY value::FLOAT8) FROM events", vec!["1", "bad"]),
    ];

    // Act / Assert
    for (stage, sql, values) in cases {
        let (result, reached, bytes) = reached_cancellation_case(sql, &values, stage);
        // Assert
        assert!(reached);
        assert!(
            matches!(result, Err(QueryError::General(ref message)) if message == "query canceled")
        );
        assert_eq!(bytes, 0);
    }
}

#[test]
fn should_preserve_current_input_parse_errors_before_retention_check() {
    // Arrange
    let cases = [
        (RetentionStage::Mode, "SELECT mode() WITHIN GROUP (ORDER BY value::FLOAT8) FROM events", vec!["bad"]),
        (RetentionStage::Direct, "SELECT rank('bad'::FLOAT8, 1::FLOAT8) WITHIN GROUP (ORDER BY value::FLOAT8, value::FLOAT8) FROM events", vec!["1"]),
        (RetentionStage::Ordered, "SELECT rank(0::FLOAT8) WITHIN GROUP (ORDER BY value::FLOAT8) FROM events", vec!["bad"]),
    ];

    // Act / Assert
    for (stage, sql, values) in cases {
        let (result, reached, bytes) = reached_cancellation_case(sql, &values, stage);
        // Assert
        assert!(!reached);
        assert!(matches!(
            result,
            Err(QueryError::Cassie(CassieError::FloatInput(_)))
        ));
        assert_eq!(bytes, 0);
    }
}
