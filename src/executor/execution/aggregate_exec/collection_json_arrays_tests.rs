//! Actual native-array collection output retains its parsed document owner.
use super::*;
use crate::types::typed::TypedValue;
use crate::types::SqlArray;

fn rows() -> Vec<BatchRow> {
    vec![BatchRow::new(vec![(
        "payload".into(),
        Value::Array(
            SqlArray::from_elements(vec![
                Value::Typed(TypedValue::Numeric(
                    crate::types::extended::numeric::parse_text("9007199254740993.125500")
                        .expect("numeric"),
                )),
                Value::Null,
            ])
            .expect("native array"),
        ),
    )])]
}

#[test]
fn should_retain_native_array_jsonb_output_through_the_last_physical_alias() {
    // Arrange
    let plan = aggregate_plan("SELECT jsonb_agg(payload) AS documents FROM events");
    let functions = HashMap::new();
    let ample = controls_with_budget(1_048_576);
    let specs = aggregate_specs(&plan);
    let output = aggregate_query_batches_serial(
        rows(),
        &specs,
        &aggregate_context(&plan, &functions, &ample),
    )
    .expect("measured output");
    let peak = ample.peak_query_memory_bytes();
    let exact = controls_with_budget(peak);
    let scarce = controls_with_budget(peak - 1);

    // Act
    let admitted = aggregate_query_batches_serial(
        rows(),
        &specs,
        &aggregate_context(&plan, &functions, &exact),
    );
    let denied = aggregate_query_batches_serial(
        rows(),
        &specs,
        &aggregate_context(&plan, &functions, &scarce),
    );
    let alias = output.clone();
    drop(output);
    let retained = ample.current_query_memory_bytes();

    // Assert
    let document = alias[0][0]
        .entries()
        .iter()
        .find(|(name, _)| name == "documents")
        .expect("aggregate document");
    let Value::Json(document) = &document.1 else {
        panic!("JSONB document carrier")
    };
    assert_eq!(
        document[0][0].as_number().expect("exact number").as_str(),
        "9007199254740993.125500"
    );
    assert!(retained > 0);
    assert!(admitted.is_ok());
    assert!(denied
        .expect_err("one byte below measured peak")
        .to_string()
        .contains("query memory budget exceeded"));
    drop(alias);
    drop(admitted);
    for controls in [&ample, &exact, &scarce] {
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
