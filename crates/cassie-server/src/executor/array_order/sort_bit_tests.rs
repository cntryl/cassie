use super::*;
use crate::config::CassieRuntimeLimits;
use crate::executor::sort::accounting;
use crate::runtime::QueryCancellationHandle;
use crate::types::extended::bit_string::BitString;
use std::time::Instant;

fn controls(budget: usize, cancel: QueryCancellationHandle) -> QueryExecutionControls {
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: budget,
        query_timeout_ms: 0,
        ..CassieRuntimeLimits::default()
    };
    QueryExecutionControls::with_cancellation(&limits, Instant::now(), cancel)
}

fn bit_type() -> DataType {
    DataType::Extended(ExtendedType::Bit { length: None })
}
fn varbit_type() -> DataType {
    DataType::Extended(ExtendedType::VarBit { length: None })
}

#[test]
fn should_match_legacy_key_for_bit_varbit_empty_with_raw_padding() {
    // Arrange
    let controls = controls(1_000_000, QueryCancellationHandle::new());
    let padded: BitString =
        serde_json::from_value(serde_json::json!({"len": 1, "bytes": [255]})).expect("raw padding");
    let values = [
        Value::Typed(TypedValue::Bit(BitString::from_bits([]))),
        Value::Typed(TypedValue::Bit(padded.clone())),
        Value::Typed(TypedValue::VarBit(padded)),
    ];
    for (index, value) in values.into_iter().enumerate() {
        let data_type = if index == 2 {
            varbit_type()
        } else {
            bit_type()
        };
        assert!(eligible(&value, Some(&data_type)));
        let bound = accounting::conversion_bytes(&value, Some(&data_type)).expect("bound");
        let owner = controls.reserve_query_memory(bound).expect("admission");
        // Act
        let (actual, retained) = build_observing(&value, &controls, |_| {}).expect("bit key");
        // Assert
        assert_eq!(actual, SemanticValue::from_value(&value));
        assert!(retained <= bound);
        drop(actual);
        drop(owner);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_cancel_after_a_real_bit_byte_chunk() {
    // Arrange
    let cancel = QueryCancellationHandle::new();
    let controls = controls(1_000_000, cancel.clone());
    let value = Value::Typed(TypedValue::Bit(BitString::from_bits(std::iter::repeat_n(
        true, 65_536,
    ))));
    let bound = accounting::conversion_bytes(&value, Some(&bit_type())).expect("bound");
    let owner = controls.reserve_query_memory(bound).expect("admission");
    let mut copied = 0;
    // Act
    let result = build_observing(&value, &controls, |_| {
        copied += 1;
        cancel.cancel();
    });
    // Assert
    assert_eq!(copied, 1);
    assert!(matches!(result, Err(CassieError::QueryCanceled)));
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_untyped_with_unrelated_type_fallbacks() {
    // Arrange
    let value = Value::Typed(TypedValue::Bit(BitString::from_bits([true])));
    // Act / Assert
    // Assert
    assert!(!eligible(&value, None));
    assert!(!eligible(&value, Some(&DataType::Text)));
}

#[test]
fn should_preserve_search_lineage_before_bit_key_builder() {
    // Arrange
    let value = Value::Typed(TypedValue::Bit(BitString::from_bits([true, false, true])));
    let data_type = bit_type();
    let mut row = crate::executor::batch::BatchRow::new(vec![
        ("b".into(), value.clone()),
        ("\0cassie_cte_lineage:walk".into(), Value::String(r#"{"search_path":[[{"String":"2"}]],"search_column":"b","search_order":"breadth_first","search_key_types":["jsonb"]}"#.into())),
    ]);
    row.set_data_types(vec![data_type.clone(), DataType::Text].into());
    let controls = controls(1_000_000, QueryCancellationHandle::new());
    let expression = crate::sql::ast::Expr::Column("b".into());
    let functions = std::collections::HashMap::new();
    // Act
    let legacy = super::super::key(&row, &expression, &value, &functions, Some(&controls))
        .expect("legacy lineage");
    let (actual, retained) =
        super::super::sort_bit_key(&row, &expression, &value, &functions, &data_type, &controls)
            .expect("controlled lineage");
    // Assert
    assert!(matches!(&actual, SemanticValue::Record(_)));
    assert!(retained.is_none());
    assert_eq!(actual, legacy);
    drop(actual);
    drop(legacy);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
