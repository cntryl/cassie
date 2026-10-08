use super::controls;
use crate::executor::batch::BatchRow;
use crate::executor::typed_batch::row_bridge::from_rows;
use crate::types::{DataType, Value};

#[test]
fn should_infer_selected_unknown_primitive_carriers_without_promoting_output_types() {
    // Arrange
    for (values, data_type) in [
        (vec![Value::Null, Value::Null], DataType::Null),
        (
            vec![Value::Null, Value::Int64(9_007_199_254_740_993)],
            DataType::BigInt,
        ),
        (vec![Value::Null, Value::Float64(-0.0)], DataType::Float),
        (vec![Value::Null, Value::Bool(true)], DataType::Boolean),
    ] {
        let controls = controls();
        let rows = values
            .iter()
            .cloned()
            .map(|value| BatchRow::new(vec![("n".into(), value)]))
            .collect::<Vec<_>>();
        // Act
        let batch = from_rows(&controls, &rows, &[0])
            .expect("eligibility")
            .expect("native primitive");
        // Assert
        assert_eq!(batch.schema()[0].1, data_type);
        for (lane, value) in values.iter().enumerate() {
            assert_eq!(&batch.cell(0, lane).expect("cell").to_owned(), value);
        }
        assert!(rows.iter().all(|row| row.data_types().is_empty()));
        drop(batch);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_decline_unselected_null_first_keys_before_native_conversion() {
    // Arrange
    for values in [
        vec![
            Value::Null,
            Value::Int64(9_007_199_254_740_993),
            Value::Float64(9_007_199_254_740_992.0),
        ],
        vec![Value::Null, Value::String("rich".into())],
        vec![Value::Null, Value::Json(serde_json::json!([1]))],
    ] {
        let controls = controls();
        let rows = values
            .iter()
            .cloned()
            .map(|value| BatchRow::new(vec![("n".into(), value)]))
            .collect::<Vec<_>>();
        // Act
        let native = from_rows(&controls, &rows, &[0]).expect("eligibility decline");
        // Assert
        assert!(native.is_none());
        assert_eq!(controls.current_query_memory_bytes(), 0);
        assert_eq!(
            rows.iter()
                .map(|row| row.entries()[0].1.clone())
                .collect::<Vec<_>>(),
            values
        );
    }
    let controls = controls();
    let mut row = BatchRow::new(vec![("n".into(), Value::Int64(7))]);
    row.set_data_types(std::sync::Arc::new(vec![DataType::Json]));
    assert!(from_rows(&controls, &[row], &[0])
        .expect("declared rich type")
        .is_none());
}

#[test]
fn should_preserve_input_on_bridge_admission_denial() {
    // Arrange
    let controls = controls();
    let rows = vec![BatchRow::new(vec![("n".into(), Value::Int64(7))])];
    let pressure = controls
        .reserve_query_memory(controls.query_memory_budget_bytes - 1)
        .expect("pressure");
    let before = controls.current_query_memory_bytes();
    // Act
    let result = from_rows(&controls, &rows, &[0]);
    // Assert
    assert!(matches!(
        result.map_err(crate::app::CassieError::from),
        Err(crate::app::CassieError::ResourceLimit(_))
    ));
    assert_eq!(controls.current_query_memory_bytes(), before);
    assert_eq!(rows[0].get("n"), Some(&Value::Int64(7)));
    drop(pressure);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
