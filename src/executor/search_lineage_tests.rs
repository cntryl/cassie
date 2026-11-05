use super::*;
use crate::config::CassieRuntimeLimits;
use crate::executor::batch::BatchRow;
use crate::runtime::QueryCancellationHandle;
use std::time::Instant;

fn controls(budget: usize) -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    )
}
fn row(number: &str) -> BatchRow {
    BatchRow::new(vec![
        ("sequence".into(), Value::String("(0,number)".into())),
        (
            "\0cassie_cte_lineage:walk".into(),
            Value::String(format!(
                r#"{{"search_path":[[{{"String":"{number}"}}]],"search_column":"sequence","search_order":"breadth_first","search_key_types":["jsonb"]}}"#
            )),
        ),
    ])
}
fn key(number: &str, controls: &QueryExecutionControls) -> SemanticValue {
    extract(
        &row(number),
        &Expr::Column("sequence".into()),
        Some(controls),
    )
    .expect("valid lineage")
    .expect("matching key")
}

#[test]
fn should_compare_declared_jsonb_search_lineage_using_exact_identity() {
    // Arrange
    let controls = controls(1_048_576);
    // Act
    let first = key("2.0", &controls);
    let equivalent = key("2e0", &controls);
    let later = key("10", &controls);
    // Assert
    assert_eq!(first, equivalent);
    assert!(first < later);
}

#[test]
fn should_retain_owned_search_key_backing_through_its_last_alias() {
    // Arrange
    let controls = controls(1_048_576);
    // Act
    let original = key("2.0", &controls);
    let live = controls.current_query_memory_bytes();
    let alias = original.clone();
    drop(original);
    let aliased = controls.current_query_memory_bytes();
    drop(alias);
    // Assert
    assert!(live > 0);
    assert_eq!(live, aliased);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_search_lineage_normalization_at_the_measured_boundary() {
    // Arrange
    let ample = controls(1_048_576);
    let measured = key("2.0", &ample);
    let peak = ample.peak_query_memory_bytes();
    drop(measured);
    let exact = controls(peak);
    let scarce = controls(peak - 1);
    // Act
    let admitted = extract(&row("2.0"), &Expr::Column("sequence".into()), Some(&exact));
    let denied = extract(&row("2.0"), &Expr::Column("sequence".into()), Some(&scarce));
    // Assert
    assert!(admitted.is_ok());
    assert!(matches!(denied, Err(CassieError::ResourceLimit(_))));
    drop(admitted);
    assert_eq!(exact.current_query_memory_bytes(), 0);
    assert_eq!(scarce.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_search_lineage_workspace_after_a_late_invalid_typed_key() {
    // Arrange
    let controls = controls(1_048_576);
    // Act
    let result = extract(
        &row("1e-16384"),
        &Expr::Column("sequence".into()),
        Some(&controls),
    );
    // Assert
    assert!(matches!(result,Err(CassieError::TypeInput(ref error)) if error.sqlstate=="22003"));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_search_lineage_keys_when_cancellation_follows_actual_normalization() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let mut observed = 0;
    // Act
    let result = extract_observing(
        &row("2.0"),
        &Expr::Column("sequence".into()),
        Some(&controls),
        || {
            observed += 1;
            cancellation.cancel();
        },
    );
    // Assert
    assert!(observed > 0);
    assert!(matches!(result, Err(CassieError::QueryCancelled)));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_ordinary_text_search_key_spelling() {
    // Arrange
    let controls = controls(1_048_576);
    let ordinary = |number| {
        let row = row(number);
        BatchRow::new(
            row.entries()
                .iter()
                .map(|(name, value)| {
                    let value = match value {
                        Value::String(text) if name.starts_with(PREFIX) => {
                            Value::String(text.replace("jsonb", "ordinary"))
                        }
                        value => value.clone(),
                    };
                    (name.clone(), value)
                })
                .collect(),
        )
    };
    // Act
    let ten = extract(
        &ordinary("10"),
        &Expr::Column("sequence".into()),
        Some(&controls),
    )
    .expect("ordinary key")
    .expect("SEARCH record");
    let two = extract(
        &ordinary("2"),
        &Expr::Column("sequence".into()),
        Some(&controls),
    )
    .expect("ordinary key")
    .expect("SEARCH record");
    // Assert
    assert!(ten < two);
}

#[test]
fn should_reject_misaligned_private_search_type_directories_without_leaking_memory() {
    // Arrange
    let controls = controls(1_048_576);
    let row=BatchRow::new(vec![(format!("{PREFIX}walk"),Value::String(r#"{"search_path":[[{"String":"1"},{"String":"2"}]],"search_column":"sequence","search_order":"breadth_first","search_key_types":["jsonb"]}"#.into()))]);
    // Act
    let result = extract(&row, &Expr::Column("sequence".into()), Some(&controls));
    // Assert
    assert!(matches!(result, Err(CassieError::Execution(_))));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_retain_mixed_numeric_jsonb_record_keys_after_source_parents_drop() {
    // Arrange
    let controls = controls(1_048_576);
    let record = |number: &str, jsonb: &str| {
        let numeric = Value::Typed(crate::types::typed::TypedValue::Numeric(
            crate::types::extended::numeric::parse_text(number).expect("numeric"),
        ));
        let json=serde_json::json!({"search_path":[[numeric,Value::String(jsonb.into())]],"search_column":"sequence","search_order":"breadth_first","search_key_types":["ordinary","jsonb"]}).to_string();
        BatchRow::new(vec![(format!("{PREFIX}walk"), Value::String(json))])
    };
    // Act
    let first = extract(
        &record("123456789012345678901234567890.0100", "2.0"),
        &Expr::Column("sequence".into()),
        Some(&controls),
    )
    .expect("first record")
    .expect("SEARCH record");
    let second = extract(
        &record("123456789012345678901234567890.01000", "2e0"),
        &Expr::Column("sequence".into()),
        Some(&controls),
    )
    .expect("second record")
    .expect("SEARCH record");
    let alias = first.clone();
    drop(first);
    // Assert
    assert_eq!(alias, second);
    assert!(controls.current_query_memory_bytes() > 0);
    drop(alias);
    drop(second);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_sql_array_bounds_in_owned_jsonb_search_records() {
    // Arrange
    let controls = controls(1_048_576);
    let record = |bound, text: &str| {
        let value = Value::Array(
            crate::types::SqlArray::new(
                vec![crate::types::ArrayDimension {
                    length: 2,
                    lower_bound: bound,
                }],
                vec![Value::String(text.into()), Value::Null],
            )
            .expect("array"),
        );
        let json=serde_json::json!({"search_path":[[value]],"search_column":"sequence","search_order":"breadth_first","search_key_types":["jsonb_array"]}).to_string();
        BatchRow::new(vec![(format!("{PREFIX}walk"), Value::String(json))])
    };
    // Act
    let first = extract(
        &record(-2, "2.0"),
        &Expr::Column("sequence".into()),
        Some(&controls),
    )
    .expect("first")
    .expect("record");
    let same = extract(
        &record(-2, "2e0"),
        &Expr::Column("sequence".into()),
        Some(&controls),
    )
    .expect("same")
    .expect("record");
    let shifted = extract(
        &record(-1, "2.0"),
        &Expr::Column("sequence".into()),
        Some(&controls),
    )
    .expect("shifted")
    .expect("record");
    // Assert
    assert_eq!(first, same);
    assert_ne!(first, shifted);
}
