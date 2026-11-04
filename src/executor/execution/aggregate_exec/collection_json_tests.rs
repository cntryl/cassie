//! Actual collection conversion, output ownership and admission boundaries.

use super::*;
use crate::runtime::QueryCancellationHandle;
use crate::types::typed::TypedValue;

fn numeric(value: &str) -> Value {
    Value::Typed(TypedValue::Numeric(
        crate::types::extended::numeric::parse_text(value).expect("numeric fixture"),
    ))
}

fn json_rows() -> Vec<BatchRow> {
    vec![
        BatchRow::new(vec![("payload".into(), numeric("9007199254740993.125500"))]),
        BatchRow::new(vec![("payload".into(), Value::Null)]),
    ]
}

#[test]
fn should_retain_json_aggregate_output_memory_until_the_last_row_owner_drops() {
    // Arrange
    let plan = aggregate_plan("SELECT jsonb_agg(payload) AS documents FROM events");
    let functions = HashMap::new();
    let controls = controls_with_budget(1_048_576);
    let context = aggregate_context(&plan, &functions, &controls);

    // Act
    let output = aggregate_query_batches_serial(json_rows(), &aggregate_specs(&plan), &context)
        .expect("admitted collection output");
    let retained = controls.current_query_memory_bytes();
    let clone = output.clone();
    drop(output);
    let retained_by_clone = controls.current_query_memory_bytes();
    drop(clone);

    // Assert
    assert!(
        retained > 0,
        "completed physical rows still own their JSON buffers"
    );
    assert_eq!(retained_by_clone, retained);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_exact_json_collection_output_at_the_measured_memory_boundary() {
    // Arrange
    let plan = aggregate_plan("SELECT json_agg(payload), jsonb_agg(payload) FROM events");
    let functions = HashMap::new();
    let ample = controls_with_budget(1_048_576);

    // Act
    let measured = aggregate_query_batches_serial(
        json_rows(),
        &aggregate_specs(&plan),
        &aggregate_context(&plan, &functions, &ample),
    )
    .expect("measure real collection path");
    let peak = ample.peak_query_memory_bytes();
    drop(measured);
    let exact = controls_with_budget(peak);
    let scarce = controls_with_budget(peak - 1);
    let admitted = aggregate_query_batches_serial(
        json_rows(),
        &aggregate_specs(&plan),
        &aggregate_context(&plan, &functions, &exact),
    );
    let denied = aggregate_query_batches_serial(
        json_rows(),
        &aggregate_specs(&plan),
        &aggregate_context(&plan, &functions, &scarce),
    );

    // Assert
    assert!(admitted.is_ok(), "exact peak admits the same finite result");
    assert!(denied
        .expect_err("one byte below real peak fails")
        .to_string()
        .contains("query memory budget exceeded"));
    assert_eq!(exact.peak_query_memory_bytes(), peak);
    drop(admitted);
    for controls in [&ample, &exact, &scarce] {
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_release_json_output_workspace_when_cancellation_follows_actual_value_encoding() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let values = vec![(numeric("9007199254740993.125500"), None)];
    let mut encoded = false;

    // Act
    let result = super::super::collection_json::finish_with_observer(
        crate::sql::functions::CollectionAggregate::Json {
            binary: true,
            strict: false,
        },
        values,
        true,
        super::super::collection_json::ConversionContext {
            value_type: None,
            controls: Some(&controls),
        },
        || {
            encoded = true;
            cancellation.cancel();
        },
    );

    // Assert
    assert!(encoded, "cancellation follows the real NUMERIC encoder");
    assert!(result
        .expect_err("completed input conversion is interrupted before publication")
        .to_string()
        .contains("query canceled"));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_prior_json_output_copies_when_a_later_copy_cannot_be_admitted() {
    // Arrange
    let plan = aggregate_plan("SELECT jsonb_agg(payload) AS documents FROM events");
    let functions = HashMap::new();
    let budget = 1_048_576;
    let controls = controls_with_budget(budget);
    let context = aggregate_context(&plan, &functions, &controls);
    let specs = aggregate_specs(&plan);
    let values =
        super::super::modifiers::evaluate_owned(&specs[0].function, &json_rows(), &context)
            .expect("owned JSON collection value");
    let mut row = super::super::collection_json::OutputRow::new(Vec::new(), &specs, &controls)
        .expect("physical row owner");
    let mut copies = 0;
    let mut competing = None;

    // Act
    let result = row.append_with_observer(values, &["first".into(), "second".into()], || {
        copies += 1;
        competing = Some(
            controls
                .reserve_query_memory(budget - controls.current_query_memory_bytes())
                .expect("another live owner consumes remaining headroom"),
        );
    });
    drop(row);
    drop(competing);

    // Assert
    assert_eq!(
        copies, 1,
        "the first actual copy precedes later admission failure"
    );
    assert!(result
        .expect_err("second output copy exceeds remaining headroom")
        .to_string()
        .contains("query memory budget exceeded"));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_wide_numeric_json_output_before_the_value_encoder_allocates() {
    // Arrange
    let value = numeric("1e-4096");
    let kind = crate::sql::functions::CollectionAggregate::Json {
        binary: true,
        strict: false,
    };
    let scarce = controls_with_budget(8_192);
    let ample = controls_with_budget(4_194_304);
    let mut scarce_encoded = false;
    let mut ample_encoded = false;

    // Act
    let denied = super::super::collection_json::finish_with_observer(
        kind,
        vec![(value.clone(), None)],
        true,
        super::super::collection_json::ConversionContext {
            value_type: None,
            controls: Some(&scarce),
        },
        || scarce_encoded = true,
    );
    let accepted = super::super::collection_json::finish_with_observer(
        kind,
        vec![(value, None)],
        true,
        super::super::collection_json::ConversionContext {
            value_type: None,
            controls: Some(&ample),
        },
        || ample_encoded = true,
    );

    // Assert
    assert!(
        !scarce_encoded,
        "workspace is denied before formatting 4096 decimal places"
    );
    assert!(denied
        .expect_err("decimal output exceeds the small budget")
        .to_string()
        .contains("query memory budget exceeded"));
    assert!(ample_encoded);
    assert!(accepted.is_ok());
    drop(accepted);
    assert_eq!(scarce.current_query_memory_bytes(), 0);
    assert_eq!(ample.current_query_memory_bytes(), 0);
}

#[test]
fn should_retain_jsonb_collection_results_as_native_document_values() {
    // Arrange
    let plan = aggregate_plan("SELECT jsonb_agg(payload) AS documents FROM events");
    let functions = HashMap::new();
    let controls = controls_with_budget(1_048_576);
    // Act
    let output = aggregate_query_batches_serial(
        json_rows(),
        &aggregate_specs(&plan),
        &aggregate_context(&plan, &functions, &controls),
    )
    .expect("native JSONB aggregate");
    // Assert
    let document = output[0][0]
        .entries()
        .iter()
        .find_map(|(_, value)| match value {
            Value::Json(document) => Some(document),
            _ => None,
        })
        .expect("JSONB physical document carrier");
    assert_eq!(
        document[0].as_number().expect("exact decimal").as_str(),
        "9007199254740993.125500"
    );
    assert!(document[1].is_null());
    assert!(
        controls.current_query_memory_bytes()
            >= crate::runtime::accounted::json::retained_bytes(document).expect("document heap")
    );
    drop(output);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_duplicate_plain_json_input_text_in_physical_aggregate_rows() {
    // Arrange
    let plan = aggregate_plan("SELECT json_agg(payload) AS documents FROM events");
    let functions = HashMap::new();
    let controls = controls_with_budget(1_048_576);
    let rows = vec![BatchRow::new(vec![(
        "payload".into(),
        Value::String(r#"{"a":1,"a":2}"#.into()),
    )])
    .with_optional_data_types(Some(std::sync::Arc::new(vec![
        crate::types::DataType::Json,
    ])))];
    // Act
    let output = aggregate_query_batches_serial(
        rows,
        &aggregate_specs(&plan),
        &aggregate_context(&plan, &functions, &controls),
    )
    .expect("plain JSON aggregate");
    // Assert
    assert!(output[0][0]
        .entries()
        .iter()
        .all(|(_, value)| *value == Value::String(r#"[{"a":1,"a":2}]"#.into())));
}

#[test]
fn should_release_parsed_jsonb_output_when_cancellation_follows_document_decode() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let document = r#"{"n":9007199254740993.125500}"#.to_owned();
    let mut memory =
        crate::executor::json_construction::reserve(Some(&controls), document.len() + 256)
            .expect("reserved construction workspace");
    let mut observed = false;
    // Act
    let result = super::super::collection_json::finish_document_observing(
        document,
        true,
        0,
        &mut memory,
        Some(&controls),
        || {
            observed = true;
            cancellation.cancel();
        },
    );
    // Assert
    assert!(observed);
    assert!(matches!(
        result,
        Err(QueryError::Cassie(crate::app::CassieError::QueryCancelled))
    ));
    drop(memory);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_parsed_jsonb_object_backing_through_its_last_output_alias() {
    // Arrange
    let plan = aggregate_plan("SELECT jsonb_object_agg(key,payload) AS documents FROM events");
    let functions = HashMap::new();
    let rows = || {
        let document =
            serde_json::from_str(r#"{"n":9007199254740993.125500,"nested":[null,{"keep":true}]}"#)
                .expect("exact nested document");
        vec![BatchRow::new(vec![
            ("key".into(), Value::String("k".repeat(64))),
            ("payload".into(), Value::Json(document)),
        ])]
    };
    let ample = controls_with_budget(4_194_304);
    let measured = aggregate_query_batches_serial(
        rows(),
        &aggregate_specs(&plan),
        &aggregate_context(&plan, &functions, &ample),
    )
    .expect("measured parsed object");
    let peak = ample.peak_query_memory_bytes();
    drop(measured);
    let exact = controls_with_budget(peak);
    let scarce = controls_with_budget(peak - 1);
    // Act
    let admitted = aggregate_query_batches_serial(
        rows(),
        &aggregate_specs(&plan),
        &aggregate_context(&plan, &functions, &exact),
    );
    let denied = aggregate_query_batches_serial(
        rows(),
        &aggregate_specs(&plan),
        &aggregate_context(&plan, &functions, &scarce),
    );
    let output = admitted.expect("exact parsed object bound");
    let alias = output.clone();
    drop(output);
    // Assert
    let document = alias[0][0]
        .entries()
        .iter()
        .find_map(|(_, value)| match value {
            Value::Json(document) => Some(document),
            _ => None,
        })
        .expect("native parsed object");
    assert_eq!(
        document["k".repeat(64)]["n"]
            .as_number()
            .expect("exact decimal")
            .as_str(),
        "9007199254740993.125500"
    );
    assert!(
        exact.current_query_memory_bytes()
            >= crate::runtime::accounted::json::retained_bytes(document)
                .expect("parsed object bound")
    );
    assert!(matches!(
        denied,
        Err(QueryError::Cassie(crate::app::CassieError::ResourceLimit(
            _
        )))
    ));
    drop(alias);
    assert_eq!(exact.current_query_memory_bytes(), 0);
    assert_eq!(scarce.current_query_memory_bytes(), 0);
}
