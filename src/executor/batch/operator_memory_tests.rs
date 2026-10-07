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
