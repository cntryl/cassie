use super::*;
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryCancellationHandle;
use crate::types::extended::{numeric, ExtendedType};
use crate::types::typed::{TypedKey, TypedValue};
use std::hash::{Hash, Hasher};
use std::time::Instant;

fn controls(budget: usize, cancel: QueryCancellationHandle) -> QueryExecutionControls {
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: budget,
        query_timeout_ms: 0,
        ..CassieRuntimeLimits::default()
    };
    QueryExecutionControls::with_cancellation(&limits, Instant::now(), cancel)
}

fn numeric_type() -> DataType {
    DataType::Extended(ExtendedType::Numeric(None))
}

fn input(text: &str) -> Value {
    Value::Typed(TypedValue::Numeric(
        numeric::parse_text(text).expect("numeric"),
    ))
}

#[test]
fn should_match_legacy_numeric_keys_and_cross_type_order() {
    // Arrange
    let controls = controls(1_000_000, QueryCancellationHandle::new());
    for text in [
        "0",
        "-0",
        "0.000",
        "1.00",
        "100e-2",
        "9223372036854775807",
        "9223372036854775808",
        "-9223372036854775808",
        "-9223372036854775809",
        "1e19",
        "-123.45000",
        "1e40",
        "-1000000000000000000000000000000000000",
        "0.0000000001",
    ] {
        let value = input(text);
        let requested = bound(&value, Some(&numeric_type()))
            .expect("bound")
            .expect("finite");
        let output_memory = controls
            .reserve_query_memory(requested)
            .expect("output admission");
        // Act
        let (actual, retained) = build_observing(&value, &controls, |_| {}).expect("key");
        let legacy = SemanticValue::from_value(&value);
        // Assert
        assert_eq!(actual, legacy);
        assert_eq!(
            retained,
            if matches!(&actual, SemanticValue::Number(_)) {
                0
            } else {
                requested
            }
        );
        assert_eq!(controls.current_query_memory_bytes(), requested);
        let mut a = std::collections::hash_map::DefaultHasher::new();
        let mut b = std::collections::hash_map::DefaultHasher::new();
        actual.hash(&mut a);
        legacy.hash(&mut b);
        assert_eq!(a.finish(), b.finish());
        for other in [
            Value::Int64(-1),
            Value::Int64(0),
            Value::Int64(1),
            Value::Float64(0.5),
        ] {
            let other = SemanticValue::from_value(&other);
            assert_eq!(actual.cmp(&other), legacy.cmp(&other));
        }
        drop(actual);
        drop(legacy);
        drop(output_memory);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_share_integer_equality_and_hash_at_i64_boundaries() {
    // Arrange
    let controls = controls(8192, QueryCancellationHandle::new());
    for (text, integer) in [
        ("0.000", 0),
        ("1.00", 1),
        ("9223372036854775807", i64::MAX),
        ("-9223372036854775808", i64::MIN),
    ] {
        let value = input(text);
        let requested = bound(&value, Some(&numeric_type()))
            .expect("bound")
            .expect("finite");
        let memory = controls.reserve_query_memory(requested).expect("output");
        // Act
        let (actual, retained) = build_observing(&value, &controls, |_| {}).expect("key");
        let integer_key = SemanticValue::from_value(&Value::Int64(integer));
        // Assert
        assert_eq!(actual, integer_key);
        assert_eq!(actual.cmp(&integer_key), std::cmp::Ordering::Equal);
        let mut a = std::collections::hash_map::DefaultHasher::new();
        let mut b = std::collections::hash_map::DefaultHasher::new();
        actual.hash(&mut a);
        integer_key.hash(&mut b);
        assert_eq!(a.finish(), b.finish());
        assert_eq!(retained, 0);
        drop(actual);
        drop(integer_key);
        drop(memory);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_preserve_public_extreme_scale_and_zero_normalization() {
    // Arrange
    let controls = controls(8192, QueryCancellationHandle::new());
    for mantissa in [num_bigint::BigInt::from(100), num_bigint::BigInt::from(0)] {
        let value = Value::Typed(TypedValue::Numeric(numeric::Numeric::Finite {
            mantissa,
            scale: u32::MAX,
        }));
        let requested = bound(&value, Some(&numeric_type()))
            .expect("bound")
            .expect("finite");
        let memory = controls.reserve_query_memory(requested).expect("output");
        // Act
        let (actual, _) = build_observing(&value, &controls, |_| {}).expect("key");
        // Assert
        assert_eq!(actual, SemanticValue::from_value(&value));
        drop(actual);
        drop(memory);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_preserve_search_lineage_before_numeric_builder() {
    // Arrange
    let value = input("100.00");
    let data_type = numeric_type();
    let mut row = crate::executor::batch::BatchRow::new(vec![
        ("n".into(), value.clone()),
        ("\0cassie_cte_lineage:walk".into(), Value::String(r#"{"search_path":[[{"String":"2"}]],"search_column":"n","search_order":"breadth_first","search_key_types":["jsonb"]}"#.into())),
    ]);
    row.set_data_types(vec![data_type.clone(), DataType::Text].into());
    let controls = controls(1_000_000, QueryCancellationHandle::new());
    let expression = crate::sql::ast::Expr::Column("n".into());
    let functions = std::collections::HashMap::new();
    // Act
    let legacy = super::super::key(&row, &expression, &value, &functions, Some(&controls))
        .expect("legacy lineage");
    let (actual, retained) = super::super::sort_numeric_key(
        &row,
        &expression,
        &value,
        &functions,
        &data_type,
        &controls,
    )
    .expect("controlled lineage");
    // Assert
    assert!(matches!(&actual, SemanticValue::Record(_)));
    assert!(retained.is_none());
    assert_eq!(actual, legacy);
    drop(actual);
    drop(legacy);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_cancel_after_real_limb_division_and_release_workspace() {
    // Arrange
    let cancel = QueryCancellationHandle::new();
    let controls = controls(1_000_000, cancel.clone());
    let value = input("12345000000000000000000000000000000000000000000000");
    let requested = bound(&value, Some(&numeric_type()))
        .expect("bound")
        .expect("finite");
    let output_memory = controls
        .reserve_query_memory(requested)
        .expect("output admission");
    let mut divided = 0;
    // Act
    let result = build_observing(&value, &controls, |phase| {
        if matches!(phase, NumericProgress::Division) {
            divided += 1;
            cancel.cancel();
        }
    });
    // Assert
    assert_eq!(divided, 1);
    assert!(matches!(result, Err(CassieError::QueryCancelled)));
    drop(output_memory);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_output_and_workspace_overlap_at_exact_budget() {
    // Arrange
    let value = input("12345000000000000000000000000000000000000000000000");
    let baseline = controls(1_000_000, QueryCancellationHandle::new());
    let requested = bound(&value, Some(&numeric_type()))
        .expect("bound")
        .expect("finite");
    let output_memory = baseline
        .reserve_query_memory(requested)
        .expect("output admission");
    let (key, _) = build_observing(&value, &baseline, |_| {}).expect("measure");
    let peak = baseline.peak_query_memory_bytes();
    drop(key);
    drop(output_memory);
    assert_eq!(baseline.current_query_memory_bytes(), 0);
    for (budget, succeeds) in [(peak, true), (peak - 1, false)] {
        let controls = controls(budget, QueryCancellationHandle::new());
        let output_memory = controls
            .reserve_query_memory(requested)
            .expect("output admission");
        // Act
        let result = build_observing(&value, &controls, |_| {});
        // Assert
        assert_eq!(result.is_ok(), succeeds);
        drop(result);
        drop(output_memory);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_keep_special_and_other_values_on_the_legacy_path() {
    // Arrange
    let special = Value::Typed(TypedValue::Numeric(numeric::Numeric::NaN));
    // Act / Assert
    assert!(bound(&special, Some(&numeric_type()))
        .expect("special")
        .is_none());
    assert!(bound(&input("1"), None).expect("untyped").is_none());
    let legacy = TypedValue::Numeric(numeric::Numeric::Infinity).key();
    assert!(matches!(
        legacy,
        TypedKey::Numeric(numeric::NumericKey::Infinity)
    ));
}
