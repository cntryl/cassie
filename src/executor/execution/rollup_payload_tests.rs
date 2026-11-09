use super::*;
use crate::config::CassieRuntimeLimits;
use crate::executor::batch::BatchRow;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use crate::types::Value;
use std::sync::Arc;
use std::time::Instant;

fn controls(budget: usize, cancellation: QueryCancellationHandle) -> QueryExecutionControls {
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: budget,
        query_timeout_ms: 0,
        ..CassieRuntimeLimits::default()
    };
    QueryExecutionControls::with_cancellation(&limits, Instant::now(), cancellation)
}

fn input(value: Value) -> BatchRow {
    BatchRow::from_projected_values(vec![("value".into(), value)])
}

#[test]
fn should_keep_query_and_operator_owners_until_each_payload_is_published() {
    // Arrange
    for (query_owned, operator_owned) in [(true, false), (false, true), (true, true)] {
        let controls = controls(8192, QueryCancellationHandle::new());
        let query = Arc::new(controls.reserve_query_memory(100).expect("query lease"));
        let operator = Arc::new(controls.reserve_query_memory(200).expect("operator lease"));
        let weak_query = Arc::downgrade(&query);
        let weak_operator = Arc::downgrade(&operator);
        let mut row = input(Value::String("payload".into()));
        if query_owned {
            row = row.with_query_memory(Some(Arc::clone(&query)));
        }
        if operator_owned {
            row = row
                .retain_operator_memory(&controls, Arc::clone(&operator))
                .expect("operator");
        }
        drop(query);
        drop(operator);
        // Act
        let mut rows = serialize_observing(vec![row], &controls, |_, _| {}).expect("payload");
        rows.publish_with(|_, payload| {
            assert_eq!(payload["value"], "payload");
            assert_eq!(weak_query.upgrade().is_some(), query_owned);
            assert_eq!(weak_operator.upgrade().is_some(), operator_owned);
            Ok(())
        })
        .expect("publish");
        // Assert
        assert!(weak_query.upgrade().is_none());
        assert!(weak_operator.upgrade().is_none());
        drop(rows);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_preserve_duplicate_name_replacement_and_field_order() {
    // Arrange
    let controls = controls(8192, QueryCancellationHandle::new());
    let row = BatchRow::from_projected_values(vec![
        ("z".into(), Value::Int64(1)),
        ("a".into(), Value::Int64(2)),
        ("z".into(), Value::Int64(3)),
    ]);
    let mut rows = serialize_observing(vec![row], &controls, |_, _| {}).expect("payload");
    // Act
    rows.publish_with(|_, payload| {
        // Assert
        assert_eq!(
            serde_json::to_string(&payload).expect("json"),
            r#"{"a":2,"z":3}"#
        );
        Ok(())
    })
    .expect("publish");
    drop(rows);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_all_owners_after_first_or_later_publication_failure() {
    // Arrange
    for fail_at in [0, 1] {
        let controls = controls(8192, QueryCancellationHandle::new());
        let mut weak = Vec::new();
        let rows = (0..2)
            .map(|_| {
                let owner = Arc::new(controls.reserve_query_memory(128).expect("owner"));
                weak.push(Arc::downgrade(&owner));
                input(Value::String("value".into())).with_query_memory(Some(owner))
            })
            .collect();
        let mut payloads = serialize_observing(rows, &controls, |_, _| {}).expect("payloads");
        // Act
        let result = payloads.publish_with(|index, _payload| {
            assert!(weak[index].upgrade().is_some());
            if index == fail_at {
                return Err(QueryError::General("injected failure".into()));
            }
            Ok(())
        });
        // Assert
        assert!(result.is_err());
        drop(payloads);
        assert!(weak.iter().all(|owner| owner.upgrade().is_none()));
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_release_source_owner_after_a_later_field_conversion_fails() {
    // Arrange
    let controls = controls(8192, QueryCancellationHandle::new());
    let owner = Arc::new(controls.reserve_query_memory(128).expect("source owner"));
    let weak = Arc::downgrade(&owner);
    let row = BatchRow::from_projected_values(vec![
        ("first".into(), Value::String("owned".into())),
        ("second".into(), Value::Float64(f64::NAN)),
    ])
    .with_query_memory(Some(owner));
    let mut fields = 0;
    // Act
    let result = serialize_observing(vec![row], &controls, |_, _| {
        fields += 1;
        assert!(weak.upgrade().is_some());
    });
    // Assert
    assert_eq!(fields, 1);
    assert!(matches!(result, Err(QueryError::General(_))));
    assert!(weak.upgrade().is_none());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_directory_at_exact_budget_and_release_on_field_cancellation() {
    // Arrange
    let bytes = 2 * std::mem::size_of::<OwnedRollupRow>();
    for (budget, succeeds) in [(bytes, true), (bytes - 1, false)] {
        let controls = controls(budget, QueryCancellationHandle::new());
        let rows = vec![input(Value::Int64(1)), input(Value::Int64(2))];
        // Act
        let result = serialize_observing(rows, &controls, |_, _| {});
        // Assert
        assert_eq!(result.is_ok(), succeeds);
        drop(result);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
    let cancellation = QueryCancellationHandle::new();
    let controls = controls(8192, cancellation.clone());
    let owner = Arc::new(controls.reserve_query_memory(128).expect("source owner"));
    let input = input(Value::String("value".into())).with_query_memory(Some(owner));
    let mut fields = 0;
    let result = serialize_observing(vec![input], &controls, |_, _| {
        fields += 1;
        cancellation.cancel();
    });
    assert_eq!(fields, 1);
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
