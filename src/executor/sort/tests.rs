use std::cell::Cell;
use std::collections::HashMap;
use std::time::Instant;

use crate::app::CassieError;
use crate::config::CassieRuntimeLimits;
use crate::executor::batch::{BatchRow, RowAccess};
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use crate::sql::ast::{Expr, OrderExpr, SortDirection};
use crate::types::Value;

use super::{
    sort_rows_with_context, sort_rows_with_controls, top_k_batches_with_context, EvalInput,
    SortRetentionContext, SortRetentionPhase,
};

struct CancellingRow<'a> {
    row: BatchRow,
    controls: &'a QueryExecutionControls,
    cancellation: &'a QueryCancellationHandle,
    key_reads: &'a Cell<usize>,
    memory_at_cancellation: &'a Cell<usize>,
}

impl RowAccess for CancellingRow<'_> {
    fn get(&self, name: &str) -> Option<&Value> {
        if name == "value" {
            let reads = self.key_reads.get() + 1;
            self.key_reads.set(reads);
            if reads == 2 {
                assert!(!self.controls.is_cancelled());
                let retained = self.controls.current_query_memory_bytes();
                assert!(retained > 0, "the first sort key must remain reserved");
                self.memory_at_cancellation.set(retained);
                self.cancellation.cancel();
            }
        }
        self.row.get(name)
    }

    fn entries(&self) -> &[(String, Value)] {
        self.row.entries()
    }
}

#[test]
fn should_cancel_sort_after_retaining_the_first_key() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits {
            query_timeout_ms: 0,
            query_memory_budget_bytes: 1024 * 1024,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
        cancellation.clone(),
    );
    assert!(controls.deadline.is_none());
    assert!(!controls.is_cancelled());
    let key_reads = Cell::new(0);
    let memory_at_cancellation = Cell::new(0);
    let rows = [3, 2, 1]
        .into_iter()
        .map(|value| CancellingRow {
            row: BatchRow::new(vec![("value".to_owned(), Value::Int64(value))]),
            controls: &controls,
            cancellation: &cancellation,
            key_reads: &key_reads,
            memory_at_cancellation: &memory_at_cancellation,
        })
        .collect();
    let order = [OrderExpr {
        expr: Expr::Column("value".to_owned()),
        direction: SortDirection::Asc,
        nulls: None,
    }];
    let functions = HashMap::new();
    let eval = EvalInput {
        order: &order,
        projection: &[],
        params: &[],
        search_context: None,
        user_functions: &functions,
        session: None,
    };

    // Act
    let result = sort_rows_with_controls(rows, &eval, &controls).map_err(CassieError::from);

    // Assert
    assert!(matches!(result, Err(CassieError::QueryCancelled)));
    assert_eq!(key_reads.get(), 2, "the third key must never be evaluated");
    assert!(memory_at_cancellation.get() > 0);
    assert!(controls.peak_query_memory_bytes() > 0);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_sort_parts_before_constructing_retained_semantic_values() {
    // Arrange
    let phase = SortRetentionPhase::SemanticPart;

    // Act
    let result = probe_retained_key(phase, false);

    // Assert
    assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
}

#[test]
fn should_admit_sort_ties_before_constructing_retained_strings() {
    // Arrange
    let phase = SortRetentionPhase::TieKey;

    // Act
    let result = probe_retained_key(phase, false);

    // Assert
    assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
}

#[test]
fn should_admit_heap_keys_before_constructing_retained_semantic_values() {
    // Arrange
    let phase = SortRetentionPhase::SemanticPart;

    // Act
    let result = probe_retained_key(phase, true);

    // Assert
    assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
}

#[test]
fn should_account_integer_formatting_overlap_when_building_a_sort_tie() {
    // Arrange
    let row = BatchRow::new(vec![
        ("first".to_owned(), Value::Int64(i64::MIN)),
        ("second".to_owned(), Value::Int64(i64::MIN)),
    ]);
    let integer_scratch = i64::MIN.to_string();

    // Act
    let estimate = super::accounting::tie_bytes(&row).expect("checked tie shape");
    let tie = crate::executor::batch::row_tie_key(&row);

    // Assert
    assert!(
        estimate >= tie.capacity() + integer_scratch.capacity(),
        "tie backing and its simultaneously live integer formatter require admission"
    );
}

#[test]
fn should_admit_inherited_array_type_clones_before_inferring_a_sort_key() {
    // Arrange
    let mut data_type = crate::types::DataType::Int;
    for _ in 0..128 {
        data_type = crate::types::DataType::Array(Box::new(data_type));
    }
    let mut outer = BatchRow::new(vec![("outer.arr".to_owned(), Value::Null)]);
    outer.set_data_types(std::sync::Arc::new(vec![data_type]));
    let row = BatchRow::new(vec![("local".to_owned(), Value::Int64(1))])
        .with_outer_scope(std::sync::Arc::new(outer));
    let expr = Expr::Column("outer.arr".to_owned());
    let functions = HashMap::new();
    assert!(row.has_array_types());

    // Act
    let estimate =
        super::accounting::type_scratch(&row, &expr, &functions).expect("type scratch shape");
    let inferred = crate::executor::array_order::expression_type(&row, &expr, &functions)
        .expect("inherited ARRAY type");
    let cloned_heap = crate::executor::retained_memory::data_type_clone_bytes(&inferred)
        .expect("recursive ARRAY clone size");

    // Assert
    assert!(
        estimate >= cloned_heap,
        "inferred outer-scope metadata must fit its preconstruction scratch bound"
    );
}

#[test]
fn should_reserve_sort_run_backing_before_its_allocation() {
    // Arrange
    let phase = SortRetentionPhase::RunBacking;

    // Act
    let calls = probe_backing_admission(phase);

    // Assert
    assert_eq!(calls, 1, "the actual run constructor must be reached");
}

#[test]
fn should_reserve_heap_backing_before_its_allocation() {
    // Arrange
    let phase = SortRetentionPhase::HeapBacking;

    // Act
    let calls = probe_backing_admission(phase);

    // Assert
    assert_eq!(calls, 1, "the actual heap constructor must be reached");
}

#[test]
fn should_allocate_only_actual_rows_for_a_small_heap_result_batch() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let functions = HashMap::new();
    let order = [OrderExpr {
        expr: Expr::IntegerLiteral(1),
        direction: SortDirection::Asc,
        nulls: None,
    }];
    let eval = EvalInput {
        order: &order,
        projection: &[],
        params: &[],
        search_context: None,
        user_functions: &functions,
        session: None,
    };

    // Act
    let batches = super::top_k_batches_with_controls(
        vec![vec![BatchRow::new(Vec::new())]],
        &eval,
        1,
        &controls,
    )
    .expect("bounded heap result");

    // Assert
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].len(), 1);
    assert_eq!(batches[0].capacity(), 1);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_cover_live_multirun_backing_at_every_merge_allocation() {
    // Arrange
    let controls = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_timeout_ms: 0,
            query_memory_budget_bytes: 1024 * 1024,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    let rows = (0_i64..513)
        .rev()
        .map(|value| {
            BatchRow::new(vec![
                ("value".to_owned(), Value::Int64(value / 3)),
                ("identity".to_owned(), Value::Int64(value)),
            ])
        })
        .collect();
    let order = [OrderExpr {
        expr: Expr::Column("value".to_owned()),
        direction: SortDirection::Asc,
        nulls: None,
    }];
    let functions = HashMap::new();
    let eval = EvalInput {
        order: &order,
        projection: &[],
        params: &[],
        search_context: None,
        user_functions: &functions,
        session: None,
    };
    let observations = Cell::new(0);
    let uneven_merge = Cell::new(false);
    let largest_merge = Cell::new(0);
    let peak_actual = Cell::new(0);
    let probe = |observed: super::retention::RunBackingObservation| {
        observations.set(observations.get() + 1);
        uneven_merge.set(uneven_merge.get() || observed.left_len != observed.right_len);
        largest_merge.set(largest_merge.get().max(observed.new_capacity));
        peak_actual.set(peak_actual.get().max(observed.actual_bytes));
        assert!(
            observed.actual_bytes <= observed.reservation_bytes,
            "actual live backing {} exceeds its retained admission {}",
            observed.actual_bytes,
            observed.reservation_bytes
        );
        assert!(controls.current_query_memory_bytes() >= observed.reservation_bytes);
        Ok(())
    };
    let retention = SortRetentionContext::with_run_probe(&probe);

    // Act
    let sorted = sort_rows_with_context(rows, &eval, &controls, &retention)
        .expect("all measured merge backing fits its live reservation");

    // Assert
    assert_eq!(sorted.len(), 513);
    assert!(sorted.windows(2).all(|rows| {
        let Value::Int64(left) = rows[0].get("value").expect("left sort value") else {
            panic!("integer sort value");
        };
        let Value::Int64(right) = rows[1].get("value").expect("right sort value") else {
            panic!("integer sort value");
        };
        left <= right
    }));
    assert_eq!(observations.get(), 512);
    assert!(uneven_merge.get(), "the final 512+1 merge must be observed");
    assert!(largest_merge.get() >= 513);
    assert!(peak_actual.get() > 0);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

fn probe_backing_admission(phase: SortRetentionPhase) -> usize {
    let controls = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_timeout_ms: 0,
            query_memory_budget_bytes: 1024 * 1024,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    let functions = HashMap::new();
    let order = [OrderExpr {
        expr: Expr::IntegerLiteral(1),
        direction: SortDirection::Asc,
        nulls: None,
    }];
    let eval = EvalInput {
        order: &order,
        projection: &[],
        params: &[],
        search_context: None,
        user_functions: &functions,
        session: None,
    };
    let required = if phase == SortRetentionPhase::RunBacking {
        std::mem::size_of::<std::collections::VecDeque<(super::RowKey, BatchRow)>>()
            + std::mem::size_of::<(super::RowKey, BatchRow)>()
    } else {
        std::mem::size_of::<super::TopCandidate>()
    };
    let calls = Cell::new(0);
    let probe = |selected| {
        if selected == phase {
            calls.set(calls.get() + 1);
            assert!(
                controls.current_query_memory_bytes() >= required,
                "the selected backing must be admitted before allocating"
            );
            return Err(crate::executor::QueryError::from(CassieError::Execution(
                "backing constructor admitted".to_owned(),
            )));
        }
        Ok(())
    };
    let retention = SortRetentionContext::with_probe(&probe);
    let result = if phase == SortRetentionPhase::RunBacking {
        sort_rows_with_context(
            vec![BatchRow::new(Vec::new())],
            &eval,
            &controls,
            &retention,
        )
        .map(drop)
    } else {
        top_k_batches_with_context(
            vec![vec![BatchRow::new(Vec::new())]],
            &eval,
            1024,
            &controls,
            &retention,
        )
        .map(drop)
    }
    .map_err(CassieError::from);
    assert!(matches!(result, Err(CassieError::Execution(_))));
    assert_eq!(controls.current_query_memory_bytes(), 0);
    calls.get()
}

fn probe_retained_key(phase: SortRetentionPhase, heap: bool) -> Result<(), CassieError> {
    let controls = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_timeout_ms: 0,
            query_memory_budget_bytes: 10 * 1024,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    let row = BatchRow::new(vec![(
        "payload".to_owned(),
        Value::String("p".repeat(8192)),
    )]);
    let input_memory = controls
        .reserve_query_memory(8192 + 512)
        .expect("input fits");
    let input_bytes = controls.current_query_memory_bytes();
    assert!(input_bytes < controls.query_memory_budget_bytes);
    assert!(controls.deadline.is_none());
    let order = [OrderExpr {
        expr: if phase == SortRetentionPhase::SemanticPart {
            Expr::StringLiteral("k".repeat(8192))
        } else {
            Expr::IntegerLiteral(1)
        },
        direction: SortDirection::Asc,
        nulls: None,
    }];
    let functions = HashMap::new();
    let eval = EvalInput {
        order: &order,
        projection: &[],
        params: &[],
        search_context: None,
        user_functions: &functions,
        session: None,
    };
    let calls = Cell::new(0);
    let probe = |selected| {
        if selected == phase {
            calls.set(calls.get() + 1);
            return Err(crate::executor::QueryError::from(CassieError::Execution(
                format!("retained sort constructor reached: {phase:?}"),
            )));
        }
        Ok(())
    };
    let retention = SortRetentionContext::with_probe(&probe);
    let result = if heap {
        top_k_batches_with_context(vec![vec![row]], &eval, 1, &controls, &retention).map(drop)
    } else {
        sort_rows_with_context(vec![row], &eval, &controls, &retention).map(drop)
    }
    .map_err(CassieError::from);
    assert_eq!(controls.current_query_memory_bytes(), input_bytes);
    assert_eq!(
        calls.get(),
        usize::from(result.is_err() && !matches!(result, Err(CassieError::ResourceLimit(_))))
    );
    drop(input_memory);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    result
}
