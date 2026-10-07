use super::*;
use crate::config::CassieRuntimeLimits;
use std::sync::Arc;
use std::time::Instant;

#[test]
fn should_preserve_operator_leases_when_reconstructing_source_rows() {
    // Arrange
    for boundary in ["qualify", "set_rekey", "combine"] {
        let controls =
            QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
        let source = Arc::new(controls.reserve_query_memory(512).expect("source lease"));
        let operator = Arc::new(controls.reserve_query_memory(1024).expect("operator lease"));
        let mut row = Some(
            BatchRow::new(vec![("n".into(), Value::Int64(7))])
                .with_query_memory(Some(source))
                .retain_operator_memory(&controls, operator)
                .expect("source"),
        );
        // Act
        let output = match boundary {
            "qualify" => qualify_row(row.take().expect("input"), "records"),
            "set_rekey" => rekey_set_rows(&["renamed".into()], vec![row.take().expect("input")])
                .pop()
                .expect("output"),
            _ => combine_rows(
                row.as_ref().expect("input"),
                &BatchRow::new(vec![("other".into(), Value::Bool(true))]),
            )
            .expect("combined output"),
        };
        drop(row);
        // Assert
        assert!(
            controls.current_query_memory_bytes() >= 512 + 1024,
            "operator source released at {boundary}"
        );
        assert_eq!(output.entries()[0].1, Value::Int64(7));
        drop(output);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
