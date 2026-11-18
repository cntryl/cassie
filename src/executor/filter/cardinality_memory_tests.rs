//! Exact native CARDINALITY routes admit native copies without decimal scratch.

use super::*;
use crate::config::CassieRuntimeLimits;
use crate::executor::batch::BatchRow;
use crate::executor::filter::{evaluate_source_argument, EvaluationMemory};
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use crate::sql::ast::{Expr, FunctionCall};
use crate::types::typed::TypedValue;
use crate::types::SqlArray;
use std::cell::RefCell;
use std::collections::HashMap;
use std::mem::{size_of, size_of_val};
use std::time::Instant;

#[derive(Clone, Copy, Default)]
struct Observation {
    copies: usize,
    copy_delta: usize,
    dispatches: usize,
    dispatch_owner_bytes: usize,
    dispatch_native_bytes: usize,
    same_owner: bool,
    directory_bytes: usize,
}

struct ActiveObservation {
    controls: QueryExecutionControls,
    base: usize,
    evidence: Observation,
}

thread_local! {
    static OBSERVATION: RefCell<Option<ActiveObservation>> = const { RefCell::new(None) };
}

struct Observe;

impl Observe {
    fn start(controls: &QueryExecutionControls) -> Self {
        OBSERVATION.with(|slot| {
            assert!(slot.borrow().is_none(), "one scoped observation");
            *slot.borrow_mut() = Some(ActiveObservation {
                controls: controls.clone(),
                base: controls.current_query_memory_bytes(),
                evidence: Observation::default(),
            });
        });
        Self
    }

    fn evidence() -> Observation {
        OBSERVATION.with(|slot| slot.borrow().as_ref().expect("active observation").evidence)
    }
}

impl Drop for Observe {
    fn drop(&mut self) {
        OBSERVATION.with(|slot| *slot.borrow_mut() = None);
    }
}

// Called by the existing test-only leaf hook immediately before .cloned().
pub(super) fn observe_leaf_copy() {
    OBSERVATION.with(|slot| {
        if let Some(active) = slot.borrow_mut().as_mut() {
            active.evidence.copies += 1;
            active.evidence.copy_delta = active
                .controls
                .current_query_memory_bytes()
                .saturating_sub(active.base);
        }
    });
}

// Called immediately before native dispatch, while owned argument values live.
pub(super) fn observe_native_dispatch(context: EvalContext<'_>, args: &[Value]) {
    OBSERVATION.with(|slot| {
        if let Some(active) = slot.borrow_mut().as_mut() {
            active.evidence.dispatches += 1;
            active.evidence.dispatch_native_bytes =
                size_of_val(args) + args.iter().map(native_bytes).sum::<usize>();
            active.evidence.dispatch_owner_bytes =
                context.memory.map_or(0, EvaluationMemory::bytes);
            active.evidence.same_owner = context
                .memory
                .is_some_and(|memory| memory.belongs_to(&active.controls));
        }
    });
}

pub(super) fn observe_argument_directory(context: EvalContext<'_>) {
    OBSERVATION.with(|slot| {
        if let Some(active) = slot.borrow_mut().as_mut() {
            active.evidence.directory_bytes = context.memory.map_or(0, EvaluationMemory::bytes);
        }
    });
}

// Independent lower bound over the actual native directories and payload.
// No formatting estimator or prospective CARDINALITY implementation is used.
fn native_bytes(value: &Value) -> usize {
    match value {
        Value::Array(array) => {
            size_of_val(array.dimensions())
                + array.len() * size_of::<Value>()
                + array.elements().iter().map(native_bytes).sum::<usize>()
        }
        Value::Typed(TypedValue::Numeric(crate::types::extended::numeric::Numeric::Finite {
            mantissa,
            ..
        })) => usize::try_from(mantissa.bits().div_ceil(8)).expect("tiny numeric fixture"),
        Value::String(text) => text.len(),
        _ => 0,
    }
}

fn numeric_array() -> Value {
    let numeric =
        crate::types::extended::numeric::parse_text(&"1".repeat(2_048)).expect("wide numeric");
    Value::Array(
        SqlArray::from_elements(vec![
            Value::Typed(TypedValue::Numeric(numeric.clone())),
            Value::Typed(TypedValue::Numeric(numeric)),
        ])
        .expect("two numeric elements"),
    )
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

fn expression(name: &str, argument: Expr) -> Expr {
    Expr::Function(Box::new(FunctionCall {
        name: name.to_owned(),
        args: vec![argument],
        ..FunctionCall::default()
    }))
}

fn evaluate(
    row: &BatchRow,
    expression: &Expr,
    params: &[Value],
    controls: &QueryExecutionControls,
    memory: &EvaluationMemory,
) -> Result<Value, QueryError> {
    evaluate_source_argument(
        row,
        expression,
        params,
        &HashMap::new(),
        None,
        controls,
        memory,
    )
}

#[test]
fn should_count_native_numeric_params_with_a_small_memory_budget() {
    // Arrange
    let controls = controls(32 * 1024);
    let params = [numeric_array()];
    let input = controls
        .reserve_query_memory(native_bytes(&params[0]))
        .expect("separate input payload owner");
    let memory = EvaluationMemory::new(&controls).expect("expression owner");
    let row = BatchRow::new(Vec::new());
    let expression = expression("cardinality", Expr::Param(0));

    // Act
    let result = evaluate(&row, &expression, &params, &controls, &memory);

    // Assert
    assert_eq!(
        result.expect("native count fits without rendering"),
        Value::Int64(2)
    );
    drop(memory);
    assert_eq!(controls.current_query_memory_bytes(), input.bytes());
    drop((params, input));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_count_native_numeric_columns_with_a_small_memory_budget() {
    // Arrange
    let controls = controls(32 * 1024);
    let value = numeric_array();
    let input = controls
        .reserve_query_memory(native_bytes(&value))
        .expect("separate row payload owner");
    let row = BatchRow::new(vec![("numbers".into(), value)]);
    let memory = EvaluationMemory::new(&controls).expect("expression owner");
    let expression = expression("cardinality", Expr::Column("numbers".into()));

    // Act
    let result = evaluate(&row, &expression, &[], &controls, &memory);

    // Assert
    assert_eq!(result.expect("native column count fits"), Value::Int64(2));
    drop(memory);
    assert_eq!(controls.current_query_memory_bytes(), input.bytes());
    drop((row, input));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_pg_catalog_cardinality_as_a_native_builtin() {
    // Arrange
    let controls = controls(32 * 1024);
    let params = [numeric_array()];
    let input = controls
        .reserve_query_memory(native_bytes(&params[0]))
        .expect("input owner");
    let memory = EvaluationMemory::new(&controls).expect("expression owner");
    let expression = expression("pg_catalog.cardinality", Expr::Param(0));
    let row = BatchRow::new(Vec::new());

    // Act
    let result = evaluate(&row, &expression, &params, &controls, &memory);

    // Assert
    assert_eq!(
        result.expect("qualified native count fits"),
        Value::Int64(2)
    );
    drop((memory, params, input));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_a_native_argument_before_copying_its_leaf() {
    // Arrange
    let controls = controls(1024 * 1024);
    let params = [numeric_array()];
    let native = native_bytes(&params[0]);
    let input = controls.reserve_query_memory(native).expect("input owner");
    let memory = EvaluationMemory::new(&controls).expect("expression owner");
    let _observation = Observe::start(&controls);
    let expression = expression("cardinality", Expr::Param(0));
    let row = BatchRow::new(Vec::new());

    // Act
    let result = evaluate(&row, &expression, &params, &controls, &memory);
    let evidence = Observe::evidence();

    // Assert
    assert_eq!(result.expect("native count"), Value::Int64(2));
    assert_eq!(evidence.copies, 1);
    assert!(
        evidence.copy_delta >= native,
        "copy must have its own admitted payload"
    );
    drop((memory, params, input));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_a_native_column_before_copying_its_leaf() {
    // Arrange
    let controls = controls(1024 * 1024);
    let value = numeric_array();
    let native = native_bytes(&value);
    let input = controls.reserve_query_memory(native).expect("input owner");
    let row = BatchRow::new(vec![("numbers".into(), value)]);
    let memory = EvaluationMemory::new(&controls).expect("expression owner");
    let _observation = Observe::start(&controls);
    let expression = expression("cardinality", Expr::Column("numbers".into()));

    // Act
    let result = evaluate(&row, &expression, &[], &controls, &memory);
    let evidence = Observe::evidence();

    // Assert
    assert_eq!(result.expect("native column count"), Value::Int64(2));
    assert_eq!(evidence.copies, 1);
    assert!(
        evidence.copy_delta >= native,
        "row copy must have its own admitted payload"
    );
    drop((memory, row, input));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_the_native_argument_directory_before_collecting_values() {
    // Arrange
    let controls = controls(1024 * 1024);
    let memory = EvaluationMemory::new(&controls).expect("expression owner");
    let _observation = Observe::start(&controls);
    let expression = expression("cardinality", Expr::Param(0));
    let params = [Value::Array(
        SqlArray::from_elements(vec![]).expect("empty array"),
    )];
    let row = BatchRow::new(Vec::new());

    // Act
    let result = evaluate(&row, &expression, &params, &controls, &memory);
    let evidence = Observe::evidence();

    // Assert
    assert_eq!(result.expect("empty native count"), Value::Int64(0));
    assert!(evidence.directory_bytes >= size_of::<Value>());
    drop(memory);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_retain_native_argument_admission_through_dispatch() {
    // Arrange
    let controls = controls(1024 * 1024);
    let params = [numeric_array()];
    let input = controls
        .reserve_query_memory(native_bytes(&params[0]))
        .expect("input owner");
    let memory = EvaluationMemory::new(&controls).expect("expression owner");
    let _observation = Observe::start(&controls);
    let expression = expression("cardinality", Expr::Param(0));
    let row = BatchRow::new(Vec::new());

    // Act
    let result = evaluate(&row, &expression, &params, &controls, &memory);
    let evidence = Observe::evidence();

    // Assert
    assert_eq!(result.expect("native count"), Value::Int64(2));
    assert_eq!(evidence.dispatches, 1);
    assert!(evidence.same_owner);
    assert!(evidence.dispatch_owner_bytes >= evidence.dispatch_native_bytes);
    drop(memory);
    assert_eq!(controls.current_query_memory_bytes(), input.bytes());
    drop((params, input));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_unadmitted_native_input_before_copying_it() {
    // Arrange
    let params = [numeric_array()];
    let native = native_bytes(&params[0]);
    let controls = controls(2 * native - 1);
    let input = controls.reserve_query_memory(native).expect("input owner");
    let memory = EvaluationMemory::new(&controls).expect("expression owner");
    let _observation = Observe::start(&controls);
    let expression = expression("cardinality", Expr::Param(0));
    let row = BatchRow::new(Vec::new());

    // Act
    let result = evaluate(&row, &expression, &params, &controls, &memory);
    let evidence = Observe::evidence();

    // Assert
    assert!(matches!(
        result,
        Err(QueryError::Cassie(crate::app::CassieError::ResourceLimit(
            _
        )))
    ));
    assert_eq!(evidence.copies, 0);
    assert_eq!(evidence.dispatches, 0);
    drop(memory);
    assert_eq!(controls.current_query_memory_bytes(), input.bytes());
    drop((params, input));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_enforce_the_native_count_memory_boundary() {
    // Arrange
    let params = [Value::Array(
        SqlArray::from_elements(vec![Value::String("tiny".into())]).expect("text array"),
    )];
    let expression = expression("cardinality", Expr::Param(0));
    let row = BatchRow::new(Vec::new());
    let baseline = controls(1024 * 1024);
    let memory = EvaluationMemory::new(&baseline).expect("baseline owner");
    assert_eq!(
        evaluate(&row, &expression, &params, &baseline, &memory).expect("baseline count"),
        Value::Int64(1)
    );
    let peak = baseline.peak_query_memory_bytes();
    drop(memory);
    assert_eq!(baseline.current_query_memory_bytes(), 0);

    // Act
    let outcomes = [(peak - 1, false), (peak, true)].map(|(budget, expected)| {
        let controls = controls(budget);
        let memory = EvaluationMemory::new(&controls).expect("bounded owner");
        let result = evaluate(&row, &expression, &params, &controls, &memory);
        drop(memory);
        assert_eq!(controls.current_query_memory_bytes(), 0);
        (result, expected)
    });

    // Assert
    for (result, accepted) in outcomes {
        if accepted {
            assert_eq!(result.expect("exact admitted boundary"), Value::Int64(1));
        } else {
            assert!(matches!(
                result,
                Err(QueryError::Cassie(crate::app::CassieError::ResourceLimit(
                    _
                )))
            ));
        }
    }
}

#[test]
fn should_cancel_native_counting_before_copying_input() {
    // Arrange
    let limits = CassieRuntimeLimits::default();
    let cancellation = QueryCancellationHandle::new();
    let controls =
        QueryExecutionControls::with_cancellation(&limits, Instant::now(), cancellation.clone());
    let params = [Value::Array(
        SqlArray::from_elements(vec![Value::Int64(1)]).expect("one integer"),
    )];
    let input = controls
        .reserve_query_memory(native_bytes(&params[0]))
        .expect("input owner");
    let memory = EvaluationMemory::new(&controls).expect("expression owner");
    let expression = expression("cardinality", Expr::Param(0));
    let row = BatchRow::new(Vec::new());
    assert_eq!(
        evaluate(&row, &expression, &params, &controls, &memory).expect("uncanceled control"),
        Value::Int64(1)
    );
    let _observation = Observe::start(&controls);
    cancellation.cancel();

    // Act
    let result = evaluate(&row, &expression, &params, &controls, &memory);
    let evidence = Observe::evidence();

    // Assert
    assert!(matches!(
        result,
        Err(QueryError::Cassie(crate::app::CassieError::QueryCanceled))
    ));
    assert_eq!(evidence.copies, 0);
    assert_eq!(evidence.dispatches, 0);
    drop(memory);
    assert_eq!(controls.current_query_memory_bytes(), input.bytes());
    drop((params, input));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
