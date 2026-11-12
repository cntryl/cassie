use super::*;
use crate::config::CassieRuntimeLimits;
use http_body_util::BodyExt;
use std::time::Instant;

#[test]
fn should_keep_numeric_error_response_admitted_until_last_body_bytes_drop() {
    // Arrange
    let controls = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: 1_048_576,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    let input = format!("{}é!", "1".repeat(4095));
    let source = crate::app::OwnedNumericLiteralError::syntax(&input, &controls).expect("source");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    // Act
    let response = numeric_error_response(&source).expect("response");
    drop(source);
    let mut body = response.into_body();
    let bytes = runtime
        .block_on(body.frame())
        .expect("frame")
        .expect("body")
        .into_data()
        .expect("data");
    drop(body);

    // Assert
    let value: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(
        value["error"],
        format!("invalid input syntax for type numeric: \"{input}\"")
    );
    assert!(controls.current_query_memory_bytes() >= bytes.len());
    drop(bytes);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_return_admitted_numeric_error_from_admin_query_worker() {
    // Arrange
    let controls = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: 1_048_576,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    let worker_controls = controls.clone();
    let cassie =
        std::sync::Arc::new(
            crate::app::Cassie::new_in_memory_with_config(
                crate::config::CassieRuntimeConfig::default(),
            )
            .expect("memory fixture"),
        );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    // Act
    let response = runtime
        .block_on(
            super::super::request_execution::run_rest_blocking_route_with_cancellation(
                cassie,
                &hyper::Method::POST,
                "/api/v1/admin/query/execute",
                Instant::now(),
                "numeric_error_response_test",
                (crate::runtime::QueryCancellationHandle::new(), true),
                move |_, _| {
                    Err(crate::app::CassieError::OwnedNumericLiteral(
                        crate::app::OwnedNumericLiteralError::syntax("bad", &worker_controls)
                            .expect("source"),
                    ))
                },
            ),
        )
        .expect("admitted error response");

    // Assert
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(controls.current_query_memory_bytes() > 0);
    drop(response);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_keep_real_sql_numeric_error_admitted_through_rest_body_drop() {
    // Arrange
    let controls = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: 1_048_576,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    let cassie = crate::app::Cassie::new_in_memory_with_config(
        crate::config::CassieRuntimeConfig::default(),
    )
    .expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("root", Some(cassie.default_database.clone()));
    let error = cassie
        .execute_sql_with_controls(
            &session,
            "SELECT CAST('not_numeric' AS NUMERIC)",
            vec![],
            crate::app::ExecutionMode::SimpleQuery,
            &controls,
        )
        .expect_err("invalid NUMERIC literal");
    let diagnostic = match &error {
        crate::app::CassieError::OwnedNumericLiteral(diagnostic) => diagnostic,
        other => panic!("expected owned numeric diagnostic, got {other:?}"),
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    // Act
    let response = numeric_error_response(diagnostic).expect("admitted response");
    drop(error);
    let mut body = response.into_body();
    let bytes = runtime
        .block_on(body.frame())
        .expect("body frame")
        .expect("body")
        .into_data()
        .expect("body data");
    drop(body);

    // Assert
    let value: serde_json::Value = serde_json::from_slice(&bytes).expect("error body");
    assert_eq!(
        value["error"],
        "invalid input syntax for type numeric: \"not_numeric\""
    );
    assert!(controls.current_query_memory_bytes() > 0);
    drop(bytes);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_encode_all_json_escapes_with_utf8_like_serde_json() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let message = "quote\" slash\\ newline\n tab\t back\x08 form\x0c nul\0 unit\x1f café 🦀";
    let expected = serde_json::to_vec(&serde_json::json!({"error": message})).expect("json");
    let mut counter = Writer {
        controls: &controls,
        bytes: None,
        length: 0,
        failure: None,
    };
    encode_error(message, &mut counter).expect("measure");
    assert_eq!(counter.length, expected.len());
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(counter.length)
        .expect("reserve output");
    let mut writer = Writer {
        controls: &controls,
        bytes: Some(&mut bytes),
        length: 0,
        failure: None,
    };

    // Act
    encode_error(message, &mut writer).expect("encode");

    // Assert
    assert_eq!(bytes, expected);
}

#[test]
fn should_preserve_legacy_admin_query_error_tuple() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let worker_controls = controls.clone();
    let cassie =
        std::sync::Arc::new(
            crate::app::Cassie::new_in_memory_with_config(
                crate::config::CassieRuntimeConfig::default(),
            )
            .expect("memory fixture"),
        );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    // Act
    let result = runtime.block_on(
        super::super::request_execution::run_rest_blocking_route_with_cancellation(
            cassie,
            &hyper::Method::POST,
            "/api/v1/admin/query/execute",
            Instant::now(),
            "legacy_numeric_error_test",
            (crate::runtime::QueryCancellationHandle::new(), false),
            move |_, _| {
                Err(crate::app::CassieError::OwnedNumericLiteral(
                    crate::app::OwnedNumericLiteralError::syntax("bad", &worker_controls)
                        .expect("source"),
                ))
            },
        ),
    );

    // Assert
    assert!(
        matches!(result, Err((StatusCode::BAD_REQUEST, message)) if message == "invalid input syntax for type numeric: \"bad\"")
    );
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_interrupt_numeric_error_encoding_during_actual_scan_with_output() {
    // Arrange
    for phase in [EncodingPhase::Scan, EncodingPhase::Emit] {
        let cancellation = crate::runtime::QueryCancellationHandle::new();
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            cancellation.clone(),
        );
        let message = "a".repeat(8192);
        let mut counter = Writer {
            controls: &controls,
            bytes: None,
            length: 0,
            failure: None,
        };
        encode_error(&message, &mut counter).expect("measure");
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(counter.length)
            .expect("reserve output");
        let mut writer = Writer {
            controls: &controls,
            bytes: Some(&mut bytes),
            length: 0,
            failure: None,
        };
        let mut observed = false;

        // Act
        let result = encode_error_observing(&message, &mut writer, |current| {
            if current == phase && !observed {
                observed = true;
                cancellation.cancel();
            }
        });

        // Assert
        assert!(observed);
        assert!(matches!(result, Err(CassieError::QueryCanceled)));
        assert!(bytes.len() < message.len());
    }
}

#[test]
fn should_admit_numeric_error_with_response_overlap_at_exact_budget() {
    // Arrange
    let generous = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: 1_048_576,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    let input = "invalid".repeat(128);
    let source = crate::app::OwnedNumericLiteralError::syntax(&input, &generous).expect("source");
    let response = numeric_error_response(&source).expect("measure");
    let peak = generous.peak_query_memory_bytes();
    drop(response);
    drop(source);
    let make_controls = |budget| {
        QueryExecutionControls::from_limits(
            &CassieRuntimeLimits {
                query_memory_budget_bytes: budget,
                ..CassieRuntimeLimits::default()
            },
            Instant::now(),
        )
    };
    let exact = make_controls(peak);
    let below = make_controls(peak - 1);
    let exact_source =
        crate::app::OwnedNumericLiteralError::syntax(&input, &exact).expect("exact source");
    let below_source =
        crate::app::OwnedNumericLiteralError::syntax(&input, &below).expect("below source");

    // Act
    let exact_response = numeric_error_response(&exact_source);
    let below_response = numeric_error_response(&below_source);

    // Assert
    assert!(exact_response.is_ok());
    assert!(matches!(below_response, Err(CassieError::ResourceLimit(_))));
    drop(exact_response);
    drop(exact_source);
    drop(below_source);
    assert_eq!(exact.current_query_memory_bytes(), 0);
    assert_eq!(below.current_query_memory_bytes(), 0);
}
