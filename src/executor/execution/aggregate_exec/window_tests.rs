//! Small frame-fold witnesses for predicate order, admission and cancellation.
use std::cell::{Cell, RefCell};
use std::time::Instant;

use super::*;
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryCancellationHandle;
use crate::sql::ast::{QueryStatement, SelectItem};
use crate::types::extended::numeric::parse_text;
use crate::types::typed::TypedValue;

#[derive(Default)]
struct Observation {
    before: Vec<usize>,
    transitions: usize,
    cancel: Option<QueryCancellationHandle>,
}
thread_local! { static OBSERVATION: RefCell<Observation> = RefCell::default(); }

pub(super) fn before_transition(controls: &QueryExecutionControls) {
    OBSERVATION.with(|state| {
        state
            .borrow_mut()
            .before
            .push(controls.current_query_memory_bytes())
    });
}
pub(super) fn after_transition() {
    OBSERVATION.with(|state| {
        let mut state = state.borrow_mut();
        state.transitions += 1;
        if let Some(cancel) = state.cancel.take() {
            cancel.cancel();
        }
    });
}
fn observed<T>(
    cancel: Option<QueryCancellationHandle>,
    run: impl FnOnce() -> T,
) -> (T, Vec<usize>, usize) {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            OBSERVATION.with(|state| *state.borrow_mut() = Observation::default());
        }
    }
    OBSERVATION.with(|state| {
        *state.borrow_mut() = Observation {
            cancel,
            ..Observation::default()
        }
    });
    let _reset = Reset;
    let result = run();
    OBSERVATION.with(|state| {
        let state = state.borrow();
        (result, state.before.clone(), state.transitions)
    })
}
fn function(sql: &str) -> WindowFunctionCall {
    let parsed = crate::sql::parse_statement(sql).expect("window SQL");
    let QueryStatement::Select(select) = parsed.statement else {
        panic!("SELECT");
    };
    let SelectItem::WindowFunction { function, .. } =
        select.projection.into_iter().next().expect("projection")
    else {
        panic!("window");
    };
    function
}
fn controls(budget: usize) -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    )
}

#[test]
fn should_evaluate_one_filter_before_both_paired_frame_arguments() {
    // Arrange
    let function = function("SELECT regr_count(y,x) FILTER(WHERE keep) OVER ()");
    let rows = [Value::Bool(false), Value::Null, Value::Bool(true)].map(|keep| {
        BatchRow::new(vec![
            ("keep".into(), keep),
            ("y".into(), Value::Float64(8.0)),
            ("x".into(), Value::Float64(4.0)),
        ])
    });
    let controls = controls(1024 * 1024);
    let output = EvaluationMemory::new(&controls).expect("output owner");
    let predicates = Cell::new(0);
    let arguments = Cell::new(0);

    // Act
    let result = evaluate(
        &function,
        &rows,
        &[],
        &controls,
        &output,
        |row, expression, _| {
            let Expr::Column(name) = expression else {
                panic!("column argument");
            };
            if name == "keep" {
                predicates.set(predicates.get() + 1);
            } else {
                arguments.set(arguments.get() + 1);
            }
            Ok(row.get(name).expect("column").clone())
        },
    );

    // Assert
    assert_eq!(result.expect("paired frame"), Value::Int64(1));
    assert_eq!(predicates.get(), 3);
    assert_eq!(arguments.get(), 2);
}

#[test]
fn should_admit_numeric_workspace_before_the_first_frame_transition() {
    // Arrange
    let function = function("SELECT var_pop(n) OVER ()");
    let input = Value::Typed(TypedValue::Numeric(
        parse_text("123456789012345678901234567890.125").expect("numeric"),
    ));
    let row = BatchRow::new(vec![("n".into(), input)]);
    let admitted = controls(1024 * 1024);
    let output = EvaluationMemory::new(&admitted).expect("output owner");

    // Act
    let (result, before, transitions) = observed(None, || {
        evaluate(&function, [&row], &[], &admitted, &output, |row, _, _| {
            Ok(row.get("n").expect("input").clone())
        })
    });
    let denied = controls(before[0] - 1);
    let denied_output = EvaluationMemory::new(&denied).expect("empty output owner");
    let (rejected, _, denied_transitions) = observed(None, || {
        evaluate(
            &function,
            [&row],
            &[],
            &denied,
            &denied_output,
            |row, _, _| Ok(row.get("n").expect("input").clone()),
        )
    });

    // Assert
    assert!(matches!(result, Ok(Value::Typed(TypedValue::Numeric(_)))));
    assert_eq!(transitions, 1);
    assert!(matches!(
        rejected,
        Err(QueryError::Cassie(crate::app::CassieError::ResourceLimit(
            _
        )))
    ));
    assert_eq!(denied_transitions, 0);
    assert_eq!(denied.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_frame_state_given_cancellation_after_a_selected_transition() {
    // Arrange
    let function = function("SELECT var_pop(n) OVER ()");
    let row = BatchRow::new(vec![("n".into(), Value::Int64(7))]);
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let output = EvaluationMemory::new(&controls).expect("output owner");

    // Act
    let (result, before, transitions) = observed(Some(cancellation), || {
        evaluate(&function, [&row], &[], &controls, &output, |row, _, _| {
            Ok(row.get("n").expect("input").clone())
        })
    });

    // Assert
    assert!(before[0] > 0);
    assert_eq!(transitions, 1);
    assert!(matches!(
        result,
        Err(QueryError::Cassie(crate::app::CassieError::QueryCancelled))
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
