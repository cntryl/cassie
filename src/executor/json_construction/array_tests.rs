//! Admission and actual-element interruption on the private native-array path.
use super::*;
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryCancellationHandle;
use crate::types::typed::TypedValue;
use crate::types::{ArrayDimension, SqlArray};
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

fn array() -> SqlArray {
    SqlArray::new(
        vec![
            ArrayDimension {
                length: 2,
                lower_bound: -2,
            },
            ArrayDimension {
                length: 2,
                lower_bound: 4,
            },
        ],
        vec![
            Value::Typed(TypedValue::Numeric(
                crate::types::extended::numeric::parse_text("9007199254740993.125500")
                    .expect("numeric"),
            )),
            Value::Typed(TypedValue::Numeric(
                crate::types::extended::numeric::Numeric::NaN,
            )),
            Value::Null,
            Value::Typed(TypedValue::Real(f32::INFINITY)),
        ],
    )
    .expect("native matrix")
}

#[test]
fn should_encode_exact_native_array_elements_without_mutating_shape() {
    // Arrange
    let array = array();
    let original = array.clone();
    let kind = DataType::Array(Box::new(DataType::Extended(
        crate::types::extended::ExtendedType::Numeric(None),
    )));
    let controls = controls(1_048_576);
    let memory = reserve(
        Some(&controls),
        arrays::bound(&array, Some(&kind), false).expect("bound"),
    )
    .expect("admission");

    // Act
    let output = arrays::encode(&array, Some(&kind), Some(&controls));

    // Assert
    assert_eq!(
        output.expect("native JSON matrix"),
        r#"[[9007199254740993.125500,"NaN"],[null,"Infinity"]]"#
    );
    assert_eq!(array, original);
    drop(memory);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_deny_native_array_workspace_before_any_element_is_encoded() {
    // Arrange
    let array = array();
    let ample = controls(1_048_576);
    let bound = arrays::bound(&array, None, false).expect("bound");
    let memory = reserve(Some(&ample), bound).expect("measured admission");
    let peak = ample.peak_query_memory_bytes();
    drop(memory);
    let exact = controls(peak);
    let scarce = controls(peak - 1);
    let mut encoded = 0;

    // Act
    let admitted =
        reserve(Some(&exact), bound).and_then(|_owner| arrays::encode(&array, None, Some(&exact)));
    let denied = reserve(Some(&scarce), bound)
        .and_then(|_owner| arrays::encode_observing(&array, None, Some(&scarce), || encoded += 1));

    // Assert
    assert!(admitted.is_ok());
    assert!(denied
        .expect_err("one byte below measured admission")
        .to_string()
        .contains("query memory budget exceeded"));
    assert_eq!(encoded, 0);
    assert_eq!(exact.peak_query_memory_bytes(), peak);
    for controls in [&ample, &exact, &scarce] {
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_release_native_array_workspace_after_actual_element_cancellation() {
    // Arrange
    let array = array();
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let mut encoded = 0;

    // Act
    let result = reserve(
        Some(&controls),
        arrays::bound(&array, None, false).expect("bound"),
    )
    .and_then(|_owner| {
        arrays::encode_observing(&array, None, Some(&controls), || {
            encoded += 1;
            cancellation.cancel();
        })
    });

    // Assert
    assert_eq!(encoded, 1);
    assert!(result
        .expect_err("cancel after first encoded element")
        .to_string()
        .contains("cancel"));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_sql_null_origins_when_rendering_document_array_elements() {
    // Arrange
    let array = SqlArray::from_elements(vec![
        Value::String(r#"{"a":1,"a":2}"#.into()),
        Value::Null,
        Value::Json(serde_json::Value::Null),
    ])
    .expect("document array");
    let kind = DataType::Array(Box::new(DataType::Json));
    let original = array.clone();

    // Act
    let output = arrays::encode(&array, Some(&kind), None);

    // Assert
    assert_eq!(
        output.expect("raw document text"),
        r#"[{"a":1,"a":2},null,null]"#
    );
    assert_eq!(array, original);
    assert!(matches!(array.elements()[1], Value::Null));
    assert!(matches!(
        array.elements()[2],
        Value::Json(serde_json::Value::Null)
    ));
}

#[test]
fn should_release_array_workspace_when_a_later_document_fails_jsonb_finalization() {
    // Arrange
    let array = SqlArray::from_elements(vec![
        Value::String(r#"{"n":1}"#.into()),
        Value::String("invalid".into()),
    ])
    .expect("raw private document carriers");
    let kind = DataType::Array(Box::new(DataType::Json));
    let controls = controls(1_048_576);
    let mut encoded = 0;

    // Act
    let result = reserve(
        Some(&controls),
        arrays::bound(&array, Some(&kind), true).expect("document bound"),
    )
    .and_then(|_owner| {
        let text = arrays::encode_observing(&array, Some(&kind), Some(&controls), || encoded += 1)?;
        finish(text, true, Some(&controls))
    });

    // Assert
    assert_eq!(encoded, 2);
    let QueryError::Cassie(CassieError::TypeInput(error)) =
        result.expect_err("later invalid document")
    else {
        panic!("typed JSONB document error")
    };
    assert_eq!(error.sqlstate, "22P02");
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_raw_array_document_fragments_before_shorter_jsonb_canonicalization() {
    // Arrange
    let raw = format!("1e{}1", "0".repeat(1024));
    let document = serde_json::from_str(&raw).expect("exact number with zero-prefixed exponent");
    let array = SqlArray::from_elements(vec![Value::Json(document)]).expect("JSONB array");
    let kind = DataType::Array(Box::new(DataType::Jsonb));
    let controls = controls(1_048_576);

    // Act
    let bound = arrays::bound(&array, Some(&kind), true).expect("raw plus canonical bound");
    let output = reserve(Some(&controls), bound).and_then(|_owner| {
        let text = arrays::encode(&array, Some(&kind), Some(&controls))?;
        finish(text, true, Some(&controls))
    });

    // Assert
    assert!(
        bound >= raw.len() + 2,
        "raw fragment and destination precede canonicalization"
    );
    assert_eq!(
        output.expect("canonical array"),
        Value::String("[10]".into())
    );
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_nested_document_arrays_at_the_selected_rank_boundary() {
    // Arrange
    let document = format!("{}1.00{}", "[".repeat(80), "]".repeat(80));
    let array = SqlArray::new(
        vec![
            ArrayDimension {
                length: 1,
                lower_bound: -1
            };
            6
        ],
        vec![Value::Json(
            serde_json::from_str(&document).expect("nested document"),
        )],
    )
    .expect("rank-six array");
    let kind = DataType::Array(Box::new(DataType::Jsonb));
    let controls = controls(1_048_576);

    // Act
    let result = reserve(
        Some(&controls),
        arrays::bound(&array, Some(&kind), true).expect("nested bound"),
    )
    .and_then(|_owner| {
        let text = arrays::encode(&array, Some(&kind), Some(&controls))?;
        finish(text, true, Some(&controls))
    });

    // Assert
    assert_eq!(
        result.expect("nested native JSONB value"),
        Value::String(format!("{}1.00{}", "[".repeat(86), "]".repeat(86)))
    );
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
