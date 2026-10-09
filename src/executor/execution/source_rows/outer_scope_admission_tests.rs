//! Observe fresh clone backing at the actual source attachment seam.
use super::{attach_outer_scope, BatchRow};
use crate::runtime::QueryExecutionControls;
use crate::types::{DataType, Value};
use std::sync::Arc;
use std::time::Instant;

#[test]
fn should_admit_fresh_outer_scope_attachment_backing() {
    // Arrange
    let controls = QueryExecutionControls::from_limits(
        &crate::config::CassieRuntimeLimits {
            query_memory_budget_bytes: 4 * 1024 * 1024,
            query_timeout_ms: 0,
            ..crate::config::CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    let (row, parent) = admitted_row(&controls);
    let before = controls.current_query_memory_bytes();
    let inner = BatchRow::new(vec![("inner.n".into(), Value::Int64(1))]);

    // Act
    let readers =
        attach_outer_scope(vec![vec![inner]], &row, &controls).expect("fresh attachment admission");
    let after = controls.current_query_memory_bytes();

    // Assert
    assert!(readers[0][0].has_outer_scope());
    assert_eq!(readers[0][0].get("outer.a"), row.get("outer.a"));
    println!(
        "scope attachment before={before} after={after} copied_body={}",
        row.owned_body_bytes().expect("actual body")
    );
    assert!(after>before,"fresh enclosing entries/lookup clone must have independent admission while its source remains live");
    drop(row);
    drop(parent);
    assert!(controls.current_query_memory_bytes() > 0);
    assert!(readers[0][0].get("outer.a").is_some());
    drop(readers);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

fn admitted_row(
    controls: &QueryExecutionControls,
) -> (BatchRow, Arc<crate::runtime::QueryMemoryReservation>) {
    let row = BatchRow::new(vec![
        (
            "outer.a".into(),
            Value::Json(serde_json::json!(["x".repeat(768), null])),
        ),
        ("outer.s".into(), Value::String("y".repeat(513))),
        ("outer.empty".into(), Value::Json(serde_json::json!([]))),
    ])
    .with_optional_data_types(Some(Arc::new(vec![
        DataType::Array(Box::new(DataType::Text)),
        DataType::Text,
        DataType::Array(Box::new(DataType::BigInt)),
    ])));
    let parent = Arc::new(
        controls
            .reserve_query_memory(row.unleased_body_bytes().expect("actual source backing"))
            .expect("source admission"),
    );
    let row = row
        .with_query_memory(Some(Arc::clone(&parent)))
        .retain_operator_memory(controls, Arc::clone(&parent))
        .expect("source owner");
    (row, parent)
}

#[test]
fn should_reject_outer_scope_attachment_below_admitted_peak() {
    // Arrange
    let limits = crate::config::CassieRuntimeLimits {
        query_memory_budget_bytes: 4 * 1024 * 1024,
        query_timeout_ms: 0,
        ..crate::config::CassieRuntimeLimits::default()
    };
    let generous = QueryExecutionControls::from_limits(&limits, Instant::now());
    let (row, parent) = admitted_row(&generous);
    let readers = attach_outer_scope(vec![vec![BatchRow::new(vec![])]], &row, &generous)
        .expect("measure actual attachment peak");
    let peak = generous.peak_query_memory_bytes();
    drop(readers);
    drop(row);
    drop(parent);
    assert_eq!(generous.current_query_memory_bytes(), 0);
    let tight = QueryExecutionControls::from_limits(
        &crate::config::CassieRuntimeLimits {
            query_memory_budget_bytes: peak - 1,
            ..limits
        },
        Instant::now(),
    );
    let (row, parent) = admitted_row(&tight);
    let original = tight.current_query_memory_bytes();

    // Act
    let result = attach_outer_scope(vec![vec![BatchRow::new(vec![])]], &row, &tight);

    // Assert
    println!(
        "attachment measured peak={peak} budget={} original={original}",
        peak - 1
    );
    assert!(matches!(
        result,
        Err(crate::executor::QueryError::Cassie(
            crate::app::CassieError::ResourceLimit(_)
        ))
    ));
    assert_eq!(tight.current_query_memory_bytes(), original);
    drop(row);
    drop(parent);
    assert_eq!(tight.current_query_memory_bytes(), 0);
}

#[test]
fn should_cancel_before_outer_scope_attachment_copy() {
    // Arrange
    let cancellation = crate::runtime::QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &crate::config::CassieRuntimeLimits {
            query_memory_budget_bytes: 4 * 1024 * 1024,
            query_timeout_ms: 0,
            ..crate::config::CassieRuntimeLimits::default()
        },
        Instant::now(),
        cancellation.clone(),
    );
    let (row, parent) = admitted_row(&controls);
    let original = controls.current_query_memory_bytes();
    cancellation.cancel();

    // Act
    let result = attach_outer_scope(vec![vec![BatchRow::new(vec![])]], &row, &controls);

    // Assert
    assert!(
        matches!(result, Err(crate::executor::QueryError::General(ref message))
        if message == "query canceled")
    );
    assert_eq!(controls.current_query_memory_bytes(), original);
    drop(row);
    drop(parent);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_warmed_outer_scope_clone_capacity() {
    // Arrange
    let controls = QueryExecutionControls::from_limits(
        &crate::config::CassieRuntimeLimits {
            query_memory_budget_bytes: 4 * 1024 * 1024,
            query_timeout_ms: 0,
            ..crate::config::CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    let mut values = Vec::with_capacity(9);
    for index in 0..5 {
        values.push((
            format!("outer.a{index}"),
            Value::Json(serde_json::json!(["x".repeat(257 + index), null])),
        ));
    }
    let mut aliases = Vec::with_capacity(13);
    for index in 0..9 {
        aliases.push((format!("outer.wide{index}"), index % 5));
    }
    let row = BatchRow::with_aliases(values, aliases).with_optional_data_types(Some(Arc::new(
        vec![DataType::Array(Box::new(DataType::Text)); 5],
    )));
    assert_eq!(row.entries().len(), 5);
    assert_eq!(row.aliases().len(), 9);
    assert_eq!(row.alias_capacity(), 13);
    assert_eq!(row.get("outer.wide8"), row.get("outer.a3"));
    // This diagnostic clone observes actual backing capacities independently
    // of the source's admission estimate; it is retired before the attachment.
    let copied_body = row
        .clone()
        .owned_body_bytes()
        .expect("observed cloned backing");
    let parent = Arc::new(
        controls
            .reserve_query_memory(row.unleased_body_bytes().expect("actual source backing"))
            .expect("source admission"),
    );
    let row = row
        .with_query_memory(Some(Arc::clone(&parent)))
        .retain_operator_memory(&controls, Arc::clone(&parent))
        .expect("source owner");
    let original = controls.current_query_memory_bytes();

    // Act
    let readers = attach_outer_scope(vec![vec![BatchRow::new(vec![])]], &row, &controls)
        .expect("admit warmed rich clone");
    let retained = controls.current_query_memory_bytes();
    drop(row);
    drop(parent);

    // Assert
    println!("warmed clone entries=5 sourcecapacity=9 aliases=9 aliascapacity=13 copiedbody={copied_body} original={original} retained={retained}");
    assert!(retained - original >= copied_body);
    assert_eq!(
        readers[0][0].get("outer.wide8"),
        Some(&Value::Json(serde_json::json!(["x".repeat(260), null])))
    );
    assert!(controls.current_query_memory_bytes() > 0);
    drop(readers);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
