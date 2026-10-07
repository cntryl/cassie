use super::BatchRow;
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryExecutionControls;
use crate::types::Value;
use std::sync::Arc;
use std::time::Instant;

#[test]
fn should_keep_source_and_operator_reservations_until_last_row_clone_drops() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let source = Arc::new(controls.reserve_query_memory(512).expect("source lease"));
    let first = Arc::new(controls.reserve_query_memory(1024).expect("first operator"));
    let second = Arc::new(
        controls
            .reserve_query_memory(2048)
            .expect("second operator"),
    );
    let row = BatchRow::new(vec![("n".into(), Value::Int64(1))]).with_query_memory(Some(source));
    // Act
    let row = row
        .retain_operator_memory(&controls, first)
        .expect("first handoff")
        .retain_operator_memory(&controls, second)
        .expect("second handoff");
    let output = row.clone();
    drop(row);
    // Assert
    assert!(output.query_memory().is_some());
    assert!(controls.current_query_memory_bytes() >= 512 + 1024 + 2048);
    assert_eq!(output.get("n"), Some(&Value::Int64(1)));
    drop(output);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_operator_leases_across_owned_and_borrowed_projection() {
    // Arrange
    use crate::sql::ast::SelectItem;
    for borrowed in [false, true] {
        let controls =
            QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
        let source = Arc::new(controls.reserve_query_memory(512).expect("source lease"));
        let memory = Arc::new(controls.reserve_query_memory(1024).expect("operator lease"));
        let row = BatchRow::new(vec![("n".into(), Value::Int64(7))])
            .with_query_memory(Some(source))
            .retain_operator_memory(&controls, memory)
            .expect("handoff");
        let mut projection = vec![SelectItem::Column {
            name: "n".into(),
            alias: None,
        }];
        if borrowed {
            projection.push(SelectItem::Column {
                name: "n".into(),
                alias: Some("copy".into()),
            });
        }
        // Act
        let output = crate::executor::projection::project_batches(
            vec![vec![row]],
            &projection,
            &[],
            None,
            &std::collections::HashMap::new(),
            None,
        )
        .expect("projection");
        // Assert
        assert!(
            controls.current_query_memory_bytes() >= 512 + 1024,
            "operator source released during projection: borrowed={borrowed}"
        );
        assert_eq!(output[0][0].get("n"), Some(&Value::Int64(7)));
        drop(output);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_leave_existing_operator_lease_intact_when_link_admission_fails() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let origin = Arc::new(controls.reserve_query_memory(512).expect("origin"));
    let operator = Arc::new(controls.reserve_query_memory(1024).expect("operator"));
    let mut row = BatchRow::new(vec![("n".into(), Value::Int64(7))])
        .with_query_memory(Some(origin))
        .retain_operator_memory(&controls, operator)
        .expect("first link");
    let next = Arc::new(controls.reserve_query_memory(1).expect("next reservation"));
    let current = controls.current_query_memory_bytes();
    let pressure = controls
        .reserve_query_memory(controls.query_memory_budget_bytes - current)
        .expect("pressure");
    // Act
    let result = row.attach_operator_memory(&controls, next);
    // Assert
    assert!(matches!(
        result,
        Err(crate::app::CassieError::ResourceLimit(_))
    ));
    drop(pressure);
    assert!(controls.current_query_memory_bytes() >= 1536);
    assert_eq!(row.get("n"), Some(&Value::Int64(7)));
    drop(row);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_hold_operator_parents_through_public_row_materialization() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let memory = Arc::new(controls.reserve_query_memory(1024).expect("operator"));
    let rows = vec![BatchRow::new(vec![("n".into(), Value::Int64(7))])
        .retain_operator_memory(&controls, memory)
        .expect("link")];
    // Act
    let guard = super::OperatorMemory::hold_rows(&controls, &rows).expect("materialization guard");
    let values = rows
        .into_iter()
        .map(BatchRow::into_values)
        .collect::<Vec<_>>();
    // Assert
    assert!(controls.current_query_memory_bytes() >= 1024);
    assert_eq!(values, vec![vec![Value::Int64(7)]]);
    drop(guard);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
