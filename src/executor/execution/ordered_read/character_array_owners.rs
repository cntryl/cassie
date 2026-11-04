//! Ordered reads keep logical array metadata and admitted row bodies together.
use super::retain_ordered_rows;
use crate::executor::batch::BatchRow;
use crate::runtime::accounted::AccountedVec;
use crate::runtime::QueryExecutionControls;
use crate::types::{DataType, SqlArray, Value};
use std::sync::Arc;
use std::time::Instant;

#[test]
fn should_retain_ordered_character_row_admission_until_the_output_is_released() {
    // Arrange
    let controls = QueryExecutionControls::from_limits(
        &crate::config::CassieRuntimeLimits::default(),
        Instant::now(),
    );
    let target = DataType::Array(Box::new(DataType::Char { length: Some(3) }));
    let row = BatchRow::from_projected_values(vec![(
        "items".into(),
        Value::Array(
            SqlArray::from_elements(vec![Value::String("x  ".into())]).expect("logical array"),
        ),
    )])
    .with_optional_data_types(Some(Arc::new(vec![target.clone()])));
    let bytes =
        row.owned_body_bytes().expect("owned body") + row.pending_lookup_bytes().expect("lookup");
    let mut rows = AccountedVec::try_new(&controls).expect("ordered source owner");
    rows.try_push_with(bytes, || row)
        .expect("admitted ordered row");
    let before = controls.current_query_memory_bytes();

    // Act
    let rows = retain_ordered_rows(rows, &controls).expect("retained ordered output");
    let retained = controls.current_query_memory_bytes();

    // Assert
    assert!(before > 0);
    assert!(retained >= before);
    assert_eq!(rows[0].data_types(), &[target]);
    assert!(rows[0].operator_memory().is_some());
    drop(rows);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
