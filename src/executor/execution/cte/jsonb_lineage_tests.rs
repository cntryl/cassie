use super::{
    append_recursive_metadata, encode_lineage, lineage_column_name, same_key, same_key_observing,
    Lineage,
};
use crate::config::CassieRuntimeLimits;
use crate::executor::lineage_keys::KeyType;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use crate::sql::ast::{CteCycle, Expr};
use crate::types::{ArrayDimension, DataType, FieldSchema, SqlArray, Value};
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
fn array(bound: i32, text: &str, swapped: bool) -> Value {
    let nulls = if swapped {
        [Value::Null, Value::Json(serde_json::Value::Null)]
    } else {
        [Value::Json(serde_json::Value::Null), Value::Null]
    };
    Value::Array(
        SqlArray::new(
            vec![ArrayDimension {
                length: 3,
                lower_bound: bound,
            }],
            vec![
                Value::String(text.into()),
                nulls[0].clone(),
                nulls[1].clone(),
            ],
        )
        .expect("SQL array"),
    )
}

#[test]
fn should_select_declared_jsonb_identity_for_cycle_equality() {
    // Arrange
    let controls = controls(1_048_576);
    let first = [Value::String("1.0".into())];
    let second = [Value::String("1.00".into())];
    // Act
    let jsonb = same_key(&first, &second, &[KeyType::Jsonb], &controls).expect("typed comparison");
    let text =
        same_key(&first, &second, &[KeyType::Ordinary], &controls).expect("ordinary comparison");
    // Assert
    assert!(jsonb);
    assert!(!text);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_cycle_sql_array_bounds_with_null_element_origins() {
    // Arrange
    let controls = controls(1_048_576);
    let first = [array(-1, "1.0", false)];
    let equivalent = [array(-1, "1.00", false)];
    let shifted = [array(0, "1.00", false)];
    let swapped = [array(-1, "1.00", true)];
    let types = [KeyType::JsonbArray];
    // Act
    let same = same_key(&first, &equivalent, &types, &controls).expect("equal arrays");
    let bounds = same_key(&first, &shifted, &types, &controls).expect("changed bounds");
    let origins = same_key(&first, &swapped, &types, &controls).expect("changed null origins");
    // Assert
    assert!(same);
    assert!(!bounds);
    assert!(!origins);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_cycle_null_origins_through_private_lineage_serialization() {
    // Arrange
    let controls = controls(1_048_576);
    let lineage = Lineage {
        cycle_path: vec![
            vec![Value::Null],
            vec![Value::Json(serde_json::Value::Null)],
        ],
        cycle_key_types: vec![KeyType::Jsonb],
        ..Lineage::default()
    };
    // Act
    let Value::String(text) = encode_lineage(&lineage).expect("lineage") else {
        panic!("text carrier")
    };
    let decoded = super::decode_lineage(&text).expect("decoded lineage");
    let equal = same_key(
        &decoded.cycle_path[0],
        &decoded.cycle_path[1],
        &decoded.cycle_key_types,
        &controls,
    )
    .expect("null-origin comparison");
    // Assert
    assert!(!equal);
    assert!(matches!(decoded.cycle_path[0][0], Value::Null));
    assert!(matches!(
        decoded.cycle_path[1][0],
        Value::Json(serde_json::Value::Null)
    ));
}

#[test]
fn should_cancel_cycle_comparison_after_actual_typed_equality() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let mut observed = 0;
    // Act
    let result = same_key_observing(
        &[Value::String("1.0".into())],
        &[Value::String("1.00".into())],
        &[KeyType::Jsonb],
        &controls,
        || {
            observed += 1;
            cancellation.cancel();
        },
    );
    // Assert
    assert_eq!(observed, 1);
    assert!(matches!(
        result,
        Err(super::QueryError::Cassie(
            crate::app::CassieError::QueryCancelled
        ))
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

fn recursive_rows(
    controls: &QueryExecutionControls,
) -> Result<super::RetainedRows, super::QueryError> {
    recursive_rows_value("1.00", controls)
}
fn recursive_rows_value(
    current: &str,
    controls: &QueryExecutionControls,
) -> Result<super::RetainedRows, super::QueryError> {
    let lineage = Lineage {
        cycle_path: vec![vec![Value::String("1.0".into())]],
        cycle_key_types: vec![KeyType::Jsonb],
        ..Lineage::default()
    };
    let source = vec![vec![
        ("payload".into(), Value::String(current.into())),
        (lineage_column_name("walk"), encode_lineage(&lineage)?),
    ]];
    let mut rows = super::RetainedRows::copy(&source, controls)?;
    let cycle = CteCycle {
        by: vec!["payload".into()],
        mark_column: "is_cycle".into(),
        mark_value: Expr::BoolLiteral(true),
        default_value: Expr::BoolLiteral(false),
        path_column: "path".into(),
    };
    let fields = [FieldSchema {
        name: "payload".into(),
        data_type: DataType::Jsonb,
        nullable: true,
    }];
    append_recursive_metadata(&mut rows, "walk", None, Some(&cycle), &fields, controls)?;
    Ok(rows)
}

#[test]
fn should_admit_typed_cycle_metadata_at_the_measured_boundary() {
    // Arrange
    let ample = controls(1_048_576);
    let measured = recursive_rows(&ample).expect("measured typed lineage");
    let peak = ample.peak_query_memory_bytes();
    drop(measured);
    let exact = controls(peak);
    let scarce = controls(peak - 1);
    // Act
    let admitted = recursive_rows(&exact);
    let denied = recursive_rows(&scarce);
    // Assert
    let output = admitted.expect("exact metadata boundary");
    assert!(output.rows[0]
        .iter()
        .any(|(name, value)| name == "is_cycle" && *value == Value::Bool(true)));
    assert!(matches!(
        denied,
        Err(super::QueryError::Cassie(
            crate::app::CassieError::ResourceLimit(_)
        ))
    ));
    drop(output);
    assert_eq!(exact.current_query_memory_bytes(), 0);
    assert_eq!(scarce.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_typed_cycle_metadata_after_a_late_numeric_key_error() {
    // Arrange
    let controls = controls(1_048_576);
    // Act
    let result = recursive_rows_value("1e-16384", &controls);
    // Assert
    assert!(
        matches!(result,Err(super::QueryError::Cassie(crate::app::CassieError::TypeInput(ref error))) if error.sqlstate=="22003")
    );
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

fn base_rows_with_name(
    name: &str,
    controls: &QueryExecutionControls,
) -> Result<super::RetainedRows, super::QueryError> {
    let source = vec![vec![("payload".into(), Value::String("2.0".into()))]];
    let mut rows = super::RetainedRows::copy(&source, controls)?;
    let fields = [FieldSchema {
        name: "payload".into(),
        data_type: DataType::Jsonb,
        nullable: false,
    }];
    let search = crate::sql::ast::CteSearch {
        order: crate::sql::ast::CteSearchOrder::BreadthFirst,
        by: vec!["payload".into()],
        sequence_column: name.into(),
    };
    super::append_base_metadata(&mut rows, "walk", Some(&search), None, &fields, controls)?;
    Ok(rows)
}

#[test]
fn should_admit_generated_lineage_name_growth_before_encoding_metadata() {
    // Arrange
    let name = "s".repeat(1024);
    let ample = controls(1_048_576);
    let measured = base_rows_with_name(&name, &ample).expect("generated name bound");
    let peak = ample.peak_query_memory_bytes();
    drop(measured);
    let exact = controls(peak);
    let scarce = controls(peak - 1);
    // Act
    let admitted = base_rows_with_name(&name, &exact);
    let denied = base_rows_with_name(&name, &scarce);
    // Assert
    let output = admitted.expect("exact name growth bound");
    assert!(output.rows[0].iter().any(|(field, _)| field == &name));
    assert!(matches!(
        denied,
        Err(super::QueryError::Cassie(
            crate::app::CassieError::ResourceLimit(_)
        ))
    ));
    drop(output);
    assert_eq!(exact.current_query_memory_bytes(), 0);
    assert_eq!(scarce.current_query_memory_bytes(), 0);
}
