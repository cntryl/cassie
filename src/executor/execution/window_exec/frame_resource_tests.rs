//! Frame-position admission and cooperative cancellation witnesses.
use super::*;
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryCancellationHandle;
use std::cell::RefCell;
use std::time::Instant;

thread_local! {
    static CANCEL_AFTER: RefCell<Option<(usize, QueryCancellationHandle)>> = const { RefCell::new(None) };
}

pub(super) fn after_candidate() {
    CANCEL_AFTER.with(|slot| {
        let mut slot = slot.borrow_mut();
        if let Some((remaining, cancellation)) = slot.as_mut() {
            *remaining -= 1;
            if *remaining == 0 {
                cancellation.cancel();
            }
        }
    });
}

fn call() -> WindowFunctionCall {
    let parsed = crate::sql::parse_statement(
        "SELECT sum(n) OVER (ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING)",
    )
    .expect("window SQL");
    let crate::sql::ast::QueryStatement::Select(select) = parsed.statement else {
        panic!("SELECT");
    };
    let SelectItem::WindowFunction { function, .. } =
        select.projection.into_iter().next().expect("projection")
    else {
        panic!("window");
    };
    function
}

#[test]
fn should_admit_frame_positions_before_allocating_and_hold_until_drop() {
    // Arrange
    let function = call();
    let frame = function.frame.as_ref().expect("explicit frame");
    let rows = (0..3)
        .map(|n| BatchRow::new(vec![("n".into(), Value::Int64(n))]))
        .collect::<Vec<_>>();
    let indices = [0, 1, 2];
    let functions = HashMap::new();
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let context = WindowExecutionContext {
        params: &[],
        search_context: None,
        user_functions: &functions,
        session: None,
        controls: &controls,
        evaluate: None,
        output_memory: None,
    };

    // Act
    let positions =
        frame_positions(1, &indices, &rows, &function, frame, &context).expect("admitted frame");
    let retained = controls.current_query_memory_bytes();
    drop(positions);
    let denied = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: 3 * std::mem::size_of::<usize>() - 1,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    let denied_context = WindowExecutionContext {
        controls: &denied,
        ..context
    };
    let result = frame_positions(1, &indices, &rows, &function, frame, &denied_context);

    // Assert
    assert!(retained >= 3 * std::mem::size_of::<usize>());
    assert_eq!(controls.current_query_memory_bytes(), 0);
    assert!(matches!(
        result,
        Err(QueryError::Cassie(crate::app::CassieError::ResourceLimit(
            _
        )))
    ));
    assert_eq!(denied.current_query_memory_bytes(), 0);
}

#[test]
fn should_cancel_during_unordered_frame_construction_and_release_its_reservation() {
    // Arrange
    let function = call();
    let frame = function.frame.as_ref().expect("explicit frame");
    let rows = (0..3)
        .map(|n| BatchRow::new(vec![("n".into(), Value::Int64(n))]))
        .collect::<Vec<_>>();
    let indices = [0, 1, 2];
    let functions = HashMap::new();
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let context = WindowExecutionContext {
        params: &[],
        search_context: None,
        user_functions: &functions,
        session: None,
        controls: &controls,
        evaluate: None,
        output_memory: None,
    };
    CANCEL_AFTER.with(|slot| *slot.borrow_mut() = Some((1, cancellation)));

    // Act
    let result = frame_positions(1, &indices, &rows, &function, frame, &context);
    CANCEL_AFTER.with(|slot| *slot.borrow_mut() = None);

    // Assert
    assert!(matches!(
        result,
        Err(QueryError::Cassie(crate::app::CassieError::QueryCancelled))
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_peer_groups_and_cancel_while_scanning_unordered_rows() {
    // Arrange
    let function = call();
    let rows = (0..3)
        .map(|n| BatchRow::new(vec![("n".into(), Value::Int64(n))]))
        .collect::<Vec<_>>();
    let indices = [0, 1, 2];
    let functions = HashMap::new();
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let context = WindowExecutionContext {
        params: &[],
        search_context: None,
        user_functions: &functions,
        session: None,
        controls: &controls,
        evaluate: None,
        output_memory: None,
    };
    let denied = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: 3 * std::mem::size_of::<(usize, usize)>() - 1,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    let denied_context = WindowExecutionContext {
        controls: &denied,
        ..context
    };
    let cancellation = QueryCancellationHandle::new();
    let cancelled = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let cancelled_context = WindowExecutionContext {
        controls: &cancelled,
        ..context
    };

    // Act
    let groups = peer_groups(&indices, &rows, &function, &context).expect("admitted groups");
    let retained = controls.current_query_memory_bytes();
    drop(groups);
    let rejected = peer_groups(&indices, &rows, &function, &denied_context);
    CANCEL_AFTER.with(|slot| *slot.borrow_mut() = Some((1, cancellation)));
    let interrupted = peer_groups(&indices, &rows, &function, &cancelled_context);
    CANCEL_AFTER.with(|slot| *slot.borrow_mut() = None);

    // Assert
    assert!(retained >= 3 * std::mem::size_of::<(usize, usize)>());
    assert_eq!(controls.current_query_memory_bytes(), 0);
    assert!(matches!(
        rejected,
        Err(QueryError::Cassie(crate::app::CassieError::ResourceLimit(
            _
        )))
    ));
    assert!(matches!(
        interrupted,
        Err(QueryError::Cassie(crate::app::CassieError::QueryCancelled))
    ));
    assert_eq!(denied.current_query_memory_bytes(), 0);
    assert_eq!(cancelled.current_query_memory_bytes(), 0);
}
