use super::*;
use crate::config::CassieRuntimeLimits;
use crate::executor::batch::BatchRow;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use crate::types::{DataType, Schema, Value};
use std::sync::Arc;
use std::time::Instant;

fn controls(budget: usize, cancel: QueryCancellationHandle) -> QueryExecutionControls {
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: budget,
        query_timeout_ms: 0,
        ..CassieRuntimeLimits::default()
    };
    QueryExecutionControls::with_cancellation(&limits, Instant::now(), cancel)
}

fn schema(names: &[&str]) -> Schema {
    names
        .iter()
        .map(|name| ((*name).to_string(), DataType::Text))
        .collect()
}

fn row(value: Value) -> BatchRow {
    BatchRow::from_projected_values(vec![("input".into(), value)])
}

#[test]
fn should_keep_source_owners_through_the_batch_writer() {
    // Arrange
    for (query_owned, operator_owned) in [(true, false), (false, true), (true, true)] {
        let controls = controls(8192, QueryCancellationHandle::new());
        let query = Arc::new(controls.reserve_query_memory(100).expect("query owner"));
        let operator = Arc::new(controls.reserve_query_memory(200).expect("operator owner"));
        let weak_query = Arc::downgrade(&query);
        let weak_operator = Arc::downgrade(&operator);
        let mut input = row(Value::String("value".into()));
        if query_owned {
            input = input.with_query_memory(Some(Arc::clone(&query)));
        }
        if operator_owned {
            input = input
                .retain_operator_memory(&controls, Arc::clone(&operator))
                .expect("operator");
        }
        drop(query);
        drop(operator);
        let mut batch =
            encode_observing(vec![input], &schema(&["name"]), &controls, |_, _| {}).expect("batch");
        // Act
        batch
            .publish_with(|docs| {
                assert_eq!(docs[0].1["name"], "value");
                assert_eq!(weak_query.upgrade().is_some(), query_owned);
                assert_eq!(weak_operator.upgrade().is_some(), operator_owned);
                Ok(())
            })
            .expect("write");
        // Assert
        assert_eq!(weak_query.upgrade().is_some(), query_owned);
        assert_eq!(weak_operator.upgrade().is_some(), operator_owned);
        drop(batch);
        assert!(weak_query.upgrade().is_none());
        assert!(weak_operator.upgrade().is_none());
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_preserve_duplicate_field_replacement_with_identifier() {
    // Arrange
    let controls = controls(8192, QueryCancellationHandle::new());
    let input = BatchRow::from_projected_values(vec![
        ("x".into(), Value::Int64(1)),
        ("y".into(), Value::Int64(2)),
    ]);
    let mut batch = encode_observing(
        vec![input],
        &schema(&["same", "same"]),
        &controls,
        |_, _| {},
    )
    .expect("batch");
    // Act
    batch
        .publish_with(|docs| {
            // Assert
            assert_eq!(docs[0].1["same"], 2);
            assert_eq!(docs[0].0, super::super::deterministic_row_id(0, &docs[0].1));
            Ok(())
        })
        .expect("write");
}

#[test]
fn should_release_owners_on_width_conversion_with_batch_write_errors() {
    // Arrange
    for failure in 0..4 {
        let controls = controls(8192, QueryCancellationHandle::new());
        let mut weak = Vec::new();
        let rows = (0..2)
            .map(|index| {
                let owner = Arc::new(controls.reserve_query_memory(128).expect("owner"));
                weak.push(Arc::downgrade(&owner));
                let value = if failure == 1 && index == 1 {
                    Value::Float64(f64::NAN)
                } else {
                    Value::String("value".into())
                };
                row(value).with_query_memory(Some(owner))
            })
            .collect();
        let fields = if failure == 0 {
            schema(&["a", "b"])
        } else {
            schema(&["a"])
        };
        // Act
        let result = encode_observing(rows, &fields, &controls, |_, _| {}).and_then(|mut batch| {
            batch.publish_with(|docs| {
                for (index, _) in docs.into_iter().enumerate() {
                    assert!(weak[index].upgrade().is_some());
                    if index == failure - 2 {
                        return Err(QueryError::General("injected".into()));
                    }
                }
                Ok(())
            })
        });
        // Assert
        assert!(result.is_err());
        assert!(weak.iter().all(|owner| owner.upgrade().is_none()));
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_admit_both_directories_with_cancel_after_a_real_field() {
    // Arrange
    let bytes = 2
        * (std::mem::size_of::<(String, serde_json::Value)>()
            + std::mem::size_of::<SourceOwners>());
    for (budget, succeeds) in [(bytes, true), (bytes - 1, false)] {
        let controls = controls(budget, QueryCancellationHandle::new());
        // Act
        let result = encode_observing(
            vec![row(Value::Int64(1)), row(Value::Int64(2))],
            &schema(&["name"]),
            &controls,
            |_, _| {},
        );
        // Assert
        assert_eq!(result.is_ok(), succeeds);
        drop(result);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
    let cancel = QueryCancellationHandle::new();
    let controls = controls(8192, cancel.clone());
    let owner = Arc::new(controls.reserve_query_memory(128).expect("source"));
    let input = row(Value::String("value".into())).with_query_memory(Some(owner));
    let mut fields = 0;
    let result = encode_observing(vec![input], &schema(&["name"]), &controls, |_, _| {
        fields += 1;
        cancel.cancel();
    });
    assert_eq!(fields, 1);
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
