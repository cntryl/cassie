//! Independent six-row peer literals across selected window owner handoffs.
use super::*;
use crate::config::CassieRuntimeLimits;
use crate::types::DataType;
use std::sync::Arc;
use std::time::Instant;

fn selected_projection() -> Vec<SelectItem> {
    let statement = crate::sql::parse_statement("SELECT id,RANK() OVER (PARTITION BY g ORDER BY n) AS rank,DENSE_RANK() OVER (PARTITION BY g ORDER BY n) AS dense FROM r").expect("selected window SQL");
    crate::planner::logical::plan(&crate::sql::binder::BoundStatement {
        statement,
        indexes: Vec::new(),
    })
    .expect("logical window projection")
    .projection
}

#[test]
fn should_preserve_literal_peer_ranks_across_selected_batch_partitions() {
    // Arrange
    let projection = selected_projection();
    let mut shapes = (0..=6)
        .map(|split| vec![split, 6 - split])
        .collect::<Vec<_>>();
    shapes.extend([vec![1, 1, 1, 1, 1, 1], vec![2, 1, 2, 1]]);
    for shape in shapes {
        let controls =
            QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
        let types = Arc::new(vec![DataType::BigInt, DataType::Text, DataType::BigInt]);
        let values = [
            (1, "A", Some(5)),
            (2, "A", Some(5)),
            (3, "A", None),
            (4, "B", Some(-2)),
            (5, "B", Some(0)),
            (6, "B", Some(7)),
        ];
        let mut owners = Vec::new();
        let mut offset = 0;
        let batches = shape
            .iter()
            .map(|length| {
                let end = offset + length;
                let marker = Arc::new(
                    controls
                        .reserve_query_memory(256)
                        .expect("source lease marker"),
                );
                owners.push(Arc::downgrade(&marker));
                let rows = values[offset..end]
                    .iter()
                    .map(|(id, group, n)| {
                        BatchRow::new(vec![
                            ("id".into(), Value::Int64(*id)),
                            ("g".into(), Value::String((*group).into())),
                            ("n".into(), n.map_or(Value::Null, Value::Int64)),
                        ])
                        .with_optional_data_types(Some(Arc::clone(&types)))
                        .with_query_memory(Some(Arc::clone(&marker)))
                    })
                    .collect::<Vec<_>>();
                offset = end;
                rows
            })
            .collect::<Vec<_>>();
        assert_eq!(offset, 6);
        // Act
        let output = apply_window_functions(
            batches,
            &projection,
            &[],
            None,
            &HashMap::new(),
            None,
            &controls,
        )
        .expect("selected peer windows");
        // Assert
        let actual = output
            .iter()
            .flatten()
            .map(|row| {
                let value = |name| match row.get(name).expect("window output field") {
                    Value::Int64(value) => *value,
                    _ => panic!("BIGINT ranking carrier"),
                };
                (value("id"), value("rank"), value("dense"))
            })
            .collect::<Vec<_>>();
        assert_eq!(
            actual,
            [
                (1, 1, 1),
                (2, 1, 1),
                (3, 3, 2),
                (4, 1, 1),
                (5, 2, 2),
                (6, 3, 3)
            ]
        );
        assert!(output
            .iter()
            .flatten()
            .all(|row| row.query_memory().is_some()
                && row.operator_memory().is_some()
                && row.data_types()
                    == [
                        DataType::BigInt,
                        DataType::Text,
                        DataType::BigInt,
                        DataType::BigInt,
                        DataType::BigInt
                    ]));
        assert_eq!(
            crate::executor::typed_batch::relational_diagnostics::last_path(),
            Some(("window", "bounded_semantic_keys"))
        );
        assert!(controls.current_query_memory_bytes() > 0);
        drop(output);
        assert!(owners.iter().all(|owner| owner.upgrade().is_none()));
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
