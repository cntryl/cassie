use std::future::{poll_fn, Future};
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use tokio::io::AsyncWrite;

use super::PgWireError;
use crate::app::CassieError;
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use crate::sql::ParameterSemanticDiagnostic;

fn controls(budget: usize) -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            query_timeout_ms: 0,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    )
}

fn diagnostic() -> ParameterSemanticDiagnostic {
    ParameterSemanticDiagnostic::Inconsistent {
        slot: 0,
        previous: 16,
        requested: 25,
    }
}

#[test]
fn should_admit_hint_with_the_semantic_error_frame_at_the_exact_peak() {
    // Arrange
    let diagnostic = ParameterSemanticDiagnostic::ToCharTimeTz { qualified: false };
    let hint = "No function matches the given name and argument types. You might need to add explicit type casts.";
    let generous = controls(65_536);
    let measured =
        PgWireError::parameter_semantic(diagnostic, &generous).expect("measure hint frame");
    let peak = generous.peak_query_memory_bytes();
    drop(measured);
    let exact = controls(peak);
    let below = controls(peak - 1);

    // Act
    let success = PgWireError::parameter_semantic(diagnostic, &exact).expect("exact hint frame");
    let failure = PgWireError::parameter_semantic(diagnostic, &below);

    // Assert
    assert_eq!(success.hint.as_deref(), Some(hint));
    let encoded = success.encoded_semantic_frame().expect("owned frame");
    let field = format!("H{hint}\0");
    assert!(encoded
        .windows(field.len())
        .any(|window| window == field.as_bytes()));
    assert!(matches!(failure, Err(CassieError::ResourceLimit(_))));
    assert!(exact.current_query_memory_bytes() > 0);
    drop(success);
    assert_eq!(exact.current_query_memory_bytes(), 0);
    assert_eq!(below.current_query_memory_bytes(), 0);
}

#[test]
fn should_cancel_during_semantic_hint_copy_and_release_every_owner() {
    // Arrange
    use super::parameter_semantic::{admitted_error_frame_observing, FrameCopyPhase};
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let mut error = PgWireError::new(
        super::PgWireSeverity::Error,
        "42883",
        "missing function".to_owned(),
    );
    error.hint = Some("No function matches the given name and argument types. You might need to add explicit type casts.".to_owned());
    let memory = controls
        .reserve_query_memory(
            std::mem::size_of::<PgWireError>()
                + error.message.len()
                + error.hint.as_ref().expect("hint").len(),
        )
        .expect("owner admission");
    let mut observed = false;

    // Act
    let result = admitted_error_frame_observing(error, &controls, memory, |phase| {
        if phase == FrameCopyPhase::Hint {
            observed = true;
            cancellation.cancel();
        }
    });

    // Assert
    assert!(observed);
    assert!(matches!(result, Err(CassieError::QueryCancelled)));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_retain_semantic_error_memory_until_the_owned_error_retires() {
    // Arrange
    let controls = controls(65_536);
    // Act
    let error = Box::new(PgWireError::parameter_semantic(diagnostic(), &controls).expect("error"));
    let retained = controls.current_query_memory_bytes();
    let peak = controls.peak_query_memory_bytes();
    drop(error);
    // Assert
    assert!(retained >= std::mem::size_of::<PgWireError>());
    assert!(
        peak > retained,
        "temporary payload overlaps the retained frame"
    );
    assert_eq!(controls.current_query_memory_bytes(), 0);
    let reused = controls
        .reserve_query_memory(65_536)
        .expect("same tracker reusable");
    drop(reused);
}

#[test]
fn should_reject_semantic_error_at_one_byte_below_its_admitted_peak() {
    // Arrange
    let generous = controls(65_536);
    let error = PgWireError::parameter_semantic(diagnostic(), &generous).expect("measure error");
    let peak = generous.peak_query_memory_bytes();
    drop(error);
    let exact = controls(peak);
    let denied = controls(peak.saturating_sub(1));
    // Act
    let success = PgWireError::parameter_semantic(diagnostic(), &exact);
    let failure = PgWireError::parameter_semantic(diagnostic(), &denied);
    // Assert
    assert!(peak > 0);
    assert!(success.is_ok());
    assert!(matches!(failure, Err(CassieError::ResourceLimit(_))));
    assert_eq!(denied.current_query_memory_bytes(), 0);
    drop(success);
    assert_eq!(exact.current_query_memory_bytes(), 0);
}

#[test]
fn should_bound_undefined_function_diagnostics_given_each_arity_when_frames_are_formatted_and_retired(
) {
    // Arrange
    let cases = [
        (
            ParameterSemanticDiagnostic::UndefinedFunction {
                function: "bool_and",
                argument_count: 0,
                actual: None,
            },
            "function bool_and() does not exist",
        ),
        (
            ParameterSemanticDiagnostic::UndefinedFunction {
                function: "bool_or",
                argument_count: 1,
                actual: Some(1043),
            },
            "function bool_or(character varying) does not exist",
        ),
        (
            ParameterSemanticDiagnostic::UndefinedFunction {
                function: "every",
                argument_count: 2,
                actual: None,
            },
            "function every(...) does not exist",
        ),
    ];

    // Act
    for (diagnostic, expected) in cases {
        let generous = controls(65_536);
        let measured = PgWireError::parameter_semantic(diagnostic, &generous)
            .expect("measure undefined-function diagnostic admission");
        let peak = generous.peak_query_memory_bytes();
        drop(measured);
        let exact = controls(peak);
        let denied = controls(peak.saturating_sub(1));
        let success = PgWireError::parameter_semantic(diagnostic, &exact)
            .expect("undefined-function frame fits at its exact admitted peak");
        let failure = PgWireError::parameter_semantic(diagnostic, &denied);

        // Assert
        assert_eq!(success.code, "42883");
        assert_eq!(success.message, expected);
        assert!(success.detail.is_none());
        assert_eq!(diagnostic.message_capacity(), expected.len());
        let encoded = success
            .encoded_semantic_frame()
            .expect("admitted error retains its complete wire frame");
        let message_field = format!("M{expected}\0");
        assert_eq!(encoded.first(), Some(&b'E'));
        assert!(encoded.windows(7).any(|field| field == b"C42883\0"));
        assert!(encoded
            .windows(message_field.len())
            .any(|field| field == message_field.as_bytes()));
        assert_eq!(exact.peak_query_memory_bytes(), peak);
        assert!(exact.current_query_memory_bytes() > 0);
        assert!(matches!(failure, Err(CassieError::ResourceLimit(_))));
        assert!(denied.peak_query_memory_bytes() < peak);
        assert_eq!(denied.current_query_memory_bytes(), 0);
        assert_eq!(generous.current_query_memory_bytes(), 0);
        drop(success);
        assert_eq!(exact.current_query_memory_bytes(), 0);
        for (tracker, budget) in [(&generous, 65_536), (&exact, peak), (&denied, peak - 1)] {
            let reused = tracker
                .reserve_query_memory(budget)
                .expect("whole diagnostic budget is reusable after retirement or denial");
            drop(reused);
            assert_eq!(tracker.current_query_memory_bytes(), 0);
        }
    }
}

#[test]
fn should_bound_scalar_modifier_frames_given_a_scoped_function_when_diagnostic_admission_retires() {
    // Arrange
    let message = "FILTER specified, but modifier_private.bool_and is not an aggregate function";
    let ample = controls(65_536);
    let measured = PgWireError::nonaggregate_modifier(message, &ample)
        .expect("measure scalar modifier diagnostic");
    let peak = ample.peak_query_memory_bytes();
    drop(measured);
    let exact = controls(peak);
    let denied = controls(peak - 1);

    // Act
    let admitted = PgWireError::nonaggregate_modifier(message, &exact);
    let failure = PgWireError::nonaggregate_modifier(message, &denied);

    // Assert
    let admitted = admitted.expect("exact peak admits complete scalar modifier error");
    assert_eq!(admitted.code, "42809");
    assert_eq!(admitted.message, message);
    assert!(admitted.encoded_semantic_frame().is_some());
    assert!(matches!(failure, Err(CassieError::ResourceLimit(_))));
    assert_eq!(exact.peak_query_memory_bytes(), peak);
    assert_eq!(denied.current_query_memory_bytes(), 0);
    drop(admitted);
    assert_eq!(exact.current_query_memory_bytes(), 0);
    assert_eq!(ample.current_query_memory_bytes(), 0);
}

#[test]
fn should_format_admitted_invalid_boolean_input_as_postgres_error() {
    // Arrange
    let controls = controls(65_536);
    let input = "a\"b\n";
    // Act
    let error = PgWireError::invalid_boolean_input(input, &controls).expect("admitted error");
    let retained = controls.current_query_memory_bytes();
    // Assert
    assert_eq!(error.code, "22P02");
    assert_eq!(
        error.message,
        format!("invalid input syntax for type boolean: \"{input}\"")
    );
    assert!(error.encoded_semantic_frame().is_some());
    assert!(retained > 0);
    drop(error);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_invalid_boolean_error_frame_below_its_admitted_peak() {
    // Arrange
    let generous = controls(65_536);
    let error = PgWireError::invalid_boolean_input("maybe", &generous).expect("measure error");
    let peak = generous.peak_query_memory_bytes();
    drop(error);
    let exact = controls(peak);
    let denied = controls(peak.saturating_sub(1));
    // Act
    let success = PgWireError::invalid_boolean_input("maybe", &exact);
    let failure = PgWireError::invalid_boolean_input("maybe", &denied);
    // Assert
    assert!(peak > 0);
    assert!(success.is_ok());
    assert!(matches!(failure, Err(CassieError::ResourceLimit(_))));
    assert_eq!(denied.current_query_memory_bytes(), 0);
    drop(success);
    assert_eq!(exact.current_query_memory_bytes(), 0);
}

#[test]
fn should_prioritize_interruption_before_semantic_error_allocation() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let cancelled = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    cancellation.cancel();
    let mut expired = controls(65_536);
    expired.deadline = Some(
        Instant::now()
            .checked_sub(Duration::from_secs(1))
            .expect("past"),
    );
    // Act
    let cancelled_result = PgWireError::parameter_semantic(diagnostic(), &cancelled);
    let expired_result = PgWireError::parameter_semantic(diagnostic(), &expired);
    // Assert
    assert!(matches!(cancelled_result, Err(CassieError::QueryCancelled)));
    assert!(matches!(expired_result, Err(CassieError::DeadlineExceeded)));
    assert_eq!(cancelled.peak_query_memory_bytes(), 0);
    assert_eq!(expired.peak_query_memory_bytes(), 0);
}

struct PendingWrite {
    pending_flush: bool,
}

impl AsyncWrite for PendingWrite {
    fn poll_write(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        if self.pending_flush {
            Poll::Ready(Ok(bytes.len()))
        } else {
            Poll::Pending
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Pending
    }

    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

#[test]
fn should_retain_the_hint_owner_while_a_semantic_frame_is_pending() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    for pending_flush in [false, true] {
        let controls = controls(65_536);
        let diagnostic = ParameterSemanticDiagnostic::ToCharTimeTz { qualified: false };
        let error =
            Box::new(PgWireError::parameter_semantic(diagnostic, &controls).expect("error"));
        let retained = controls.current_query_memory_bytes();
        let mut writer = PendingWrite { pending_flush };
        let mut send = Box::pin(async move {
            super::super::writers::write_error_response(&mut writer, &error).await
        });

        // Act
        runtime.block_on(poll_fn(|context| {
            assert!(send.as_mut().poll(context).is_pending());
            Poll::Ready(())
        }));
        let pending = controls.current_query_memory_bytes();
        drop(send);

        // Assert
        assert!(retained > 0);
        assert_eq!(pending, retained);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_keep_semantic_error_admission_during_a_pending_send() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    for pending_flush in [false, true] {
        let controls = controls(65_536);
        let error =
            Box::new(PgWireError::parameter_semantic(diagnostic(), &controls).expect("error"));
        let retained = controls.current_query_memory_bytes();
        let mut writer = PendingWrite { pending_flush };
        let mut send = Box::pin(async move {
            super::super::writers::write_error_response(&mut writer, &error).await
        });
        // Act
        runtime.block_on(poll_fn(|context| {
            assert!(send.as_mut().poll(context).is_pending());
            Poll::Ready(())
        }));
        let pending = controls.current_query_memory_bytes();
        drop(send);
        // Assert
        assert!(retained > 0);
        assert_eq!(pending, retained);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_release_semantic_error_admission_after_a_failed_send() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let controls = controls(65_536);
    let error = Box::new(PgWireError::parameter_semantic(diagnostic(), &controls).expect("error"));
    let retained = controls.current_query_memory_bytes();
    let (mut writer, reader) = tokio::io::duplex(1);
    drop(reader);
    // Act
    let result = runtime.block_on(async move {
        super::super::writers::write_error_response(&mut writer, &error).await
    });
    // Assert
    assert!(retained > 0);
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_scalar_literal_frames_given_native_errors_when_exact_budgets_are_enforced() {
    // Arrange
    let cases = [
        (
            crate::sql::ScalarLiteralDiagnostic::Syntax("uuid"),
            "bad",
            "22P02",
            "invalid input syntax for type uuid: \"bad\"",
        ),
        (
            crate::sql::ScalarLiteralDiagnostic::Range("smallint"),
            "32768",
            "22003",
            "value \"32768\" is out of range for type smallint",
        ),
        (
            crate::sql::ScalarLiteralDiagnostic::FloatRange,
            "1e400",
            "22003",
            "\"1e400\" is out of range for type double precision",
        ),
    ];
    for (diagnostic, input, code, message) in cases {
        let generous = controls(65_536);
        let error =
            PgWireError::invalid_scalar_input(input, diagnostic, &generous).expect("measure error");
        let peak = generous.peak_query_memory_bytes();
        assert_eq!(error.code, code);
        assert_eq!(error.message, message);
        assert!(error.encoded_semantic_frame().is_some());
        drop(error);
        let exact = controls(peak);
        let denied = controls(peak - 1);
        // Act
        let success = PgWireError::invalid_scalar_input(input, diagnostic, &exact);
        let failure = PgWireError::invalid_scalar_input(input, diagnostic, &denied);
        // Assert
        assert!(success.is_ok());
        assert!(matches!(failure, Err(CassieError::ResourceLimit(_))));
        assert_eq!(denied.current_query_memory_bytes(), 0);
        assert_eq!(generous.current_query_memory_bytes(), 0);
        drop(success);
        assert_eq!(exact.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_prioritize_scalar_literal_cancellation_given_pending_coercion_when_allocations_are_requested(
) {
    // Arrange
    let mut parsed = crate::sql::parse_statement("SELECT text($1)").expect("type function");
    let before = serde_json::to_string(&parsed).expect("snapshot");
    let cancellation = QueryCancellationHandle::new();
    cancellation.cancel();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation,
    );
    // Act
    let normalization =
        crate::sql::parse_analysis::normalize_type_functions(&mut parsed, &[0], &controls);
    let frame = PgWireError::invalid_scalar_input(
        "bad",
        crate::sql::ScalarLiteralDiagnostic::Syntax("uuid"),
        &controls,
    );
    // Assert
    assert!(matches!(normalization, Err(CassieError::QueryCancelled)));
    assert!(matches!(frame, Err(CassieError::QueryCancelled)));
    assert_eq!(serde_json::to_string(&parsed).expect("snapshot"), before);
    assert_eq!(controls.peak_query_memory_bytes(), 0);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_copy_unknown_column_diagnostic_at_exact_peak() {
    // Arrange
    let input = "quoted_é";
    let generous = controls(65_536);
    let measured = PgWireError::copy_undefined_column(input, &generous).expect("measure");
    let peak = generous.peak_query_memory_bytes();
    drop(measured);
    let exact = controls(peak);
    let denied = controls(peak - 1);
    // Act
    let admitted = PgWireError::copy_undefined_column(input, &exact).expect("exact peak");
    let failure = PgWireError::copy_undefined_column(input, &denied);
    // Assert
    assert_eq!(admitted.code, "42703");
    assert_eq!(
        admitted.message,
        "unresolvable column reference 'quoted_é'; known fields or aliases required"
    );
    let frame = admitted.encoded_semantic_frame().expect("admitted frame");
    assert!(frame.windows(7).any(|window| window == b"C42703\0"));
    assert!(frame
        .windows("M unresolvable".len() - 1)
        .any(|window| window == b"Munresolvable"));
    assert!(matches!(failure, Err(CassieError::ResourceLimit(_))));
    assert_eq!(denied.current_query_memory_bytes(), 0);
    drop(admitted);
    assert_eq!(exact.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_jsonb_copy_diagnostic_and_frame_with_source_overlap() {
    fn prepare(
        controls: &QueryExecutionControls,
        message: &str,
    ) -> Result<(crate::runtime::QueryMemoryReservation, PgWireError), CassieError> {
        let source_memory = controls.reserve_query_memory(message.len())?;
        let frame = PgWireError::controlled_diagnostic(
            super::PgWireSeverity::Error,
            "42601",
            message,
            controls,
        )?;
        Ok((source_memory, frame))
    }
    // Arrange
    let message = "COPY binary field 1 is invalid: invalid input syntax for type jsonb: trailing comma at line 1 column 4";
    let generous = controls(65_536);
    let pair = prepare(&generous, message).expect("measure");
    let peak = generous.peak_query_memory_bytes();
    drop(pair);
    let exact = controls(peak);
    let below = controls(peak - 1);
    // Act
    let (source, frame) = prepare(&exact, message).expect("exact");
    let failure = prepare(&below, message);
    // Assert
    assert_eq!(frame.code, "42601");
    assert_eq!(frame.message, message);
    assert!(frame
        .encoded_semantic_frame()
        .expect("frame")
        .windows(7)
        .any(|window| window == b"C42601\0"));
    assert!(matches!(failure, Err(CassieError::ResourceLimit(_))));
    assert_eq!(below.current_query_memory_bytes(), 0);
    drop(frame);
    drop(source);
    assert_eq!(exact.current_query_memory_bytes(), 0);
}

#[test]
fn should_retain_jsonb_copy_diagnostic_and_frame_during_pending_write() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let controls = controls(65_536);
    let message = "COPY binary field 1 is invalid: value overflows numeric format";
    let source = controls
        .reserve_query_memory(message.len())
        .expect("source owner");
    let frame = PgWireError::controlled_diagnostic(
        super::PgWireSeverity::Error,
        "42601",
        message,
        &controls,
    )
    .expect("frame");
    let retained = controls.current_query_memory_bytes();
    let mut send = Box::pin(async move {
        let mut writer = PendingWrite {
            pending_flush: false,
        };
        let result = super::super::writers::write_error_response(&mut writer, &frame).await;
        drop(source);
        result
    });
    // Act
    runtime.block_on(poll_fn(|context| {
        assert!(send.as_mut().poll(context).is_pending());
        Poll::Ready(())
    }));
    let pending = controls.current_query_memory_bytes();
    drop(send);
    // Assert
    assert!(retained > message.len());
    assert_eq!(pending, retained);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_cancelled_copy_unknown_column_diagnostic() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    cancellation.cancel();
    // Act
    let result = PgWireError::copy_undefined_column("name", &controls);
    // Assert
    assert!(matches!(result, Err(CassieError::QueryCancelled)));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_keep_copy_unknown_column_admission_until_write_finishes() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let controls = controls(65_536);
    let error =
        Box::new(PgWireError::copy_undefined_column("missing_é", &controls).expect("error"));
    let retained = controls.current_query_memory_bytes();
    let mut writer = PendingWrite {
        pending_flush: false,
    };
    let mut send =
        Box::pin(
            async move { super::super::writers::write_error_response(&mut writer, &error).await },
        );
    // Act
    runtime.block_on(poll_fn(|context| {
        assert!(send.as_mut().poll(context).is_pending());
        Poll::Ready(())
    }));
    let pending = controls.current_query_memory_bytes();
    drop(send);
    // Assert
    assert!(retained > 0);
    assert_eq!(pending, retained);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_hold_owned_numeric_error_and_encoded_frame_through_delivery() {
    // Arrange
    let controls = controls(1_048_576);
    let input = format!("{}é!", "1".repeat(4095));
    let owned = crate::app::OwnedNumericLiteralError::syntax(&input, &controls)
        .expect("admitted diagnostic");
    let source = CassieError::OwnedNumericLiteral(owned);

    // Act
    let mapped = super::cassie_pg_error(&source);
    let frame = mapped.encoded_semantic_frame().expect("admitted frame");
    let original_retained = controls.current_query_memory_bytes();
    drop(source);

    // Assert
    assert_eq!(mapped.code, "22P02");
    assert!(frame.windows(7).any(|field| field == b"C22P02\0"));
    assert!(controls.current_query_memory_bytes() > 0);
    assert!(controls.current_query_memory_bytes() < original_retained);
    drop(mapped);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_fatal_severity_in_owned_numeric_error_frame() {
    // Arrange
    let controls = controls(65_536);
    let source = CassieError::OwnedNumericLiteral(
        crate::app::OwnedNumericLiteralError::range(&controls).expect("source"),
    );

    // Act
    let mapped = PgWireError::from_cassie_error(super::PgWireSeverity::Fatal, &source);

    // Assert
    assert!(matches!(mapped.severity, super::PgWireSeverity::Fatal));
    assert!(mapped
        .encoded_semantic_frame()
        .expect("frame")
        .windows(7)
        .any(|bytes| bytes == b"SFATAL\0"));
}

#[test]
fn should_admit_owned_numeric_frame_at_exact_overlap_budget() {
    // Arrange
    let input = "invalid".repeat(128);
    let generous = controls(1_048_576);
    let source = CassieError::OwnedNumericLiteral(
        crate::app::OwnedNumericLiteralError::syntax(&input, &generous).expect("source"),
    );
    let measured = PgWireError::controlled_diagnostic(
        super::PgWireSeverity::Error,
        "22P02",
        match &source {
            CassieError::OwnedNumericLiteral(error) => error.message(),
            _ => unreachable!(),
        },
        &generous,
    )
    .expect("measure");
    let peak = generous.peak_query_memory_bytes();
    drop(measured);
    drop(source);
    let exact = controls(peak);
    let below = controls(peak - 1);
    let exact_source =
        crate::app::OwnedNumericLiteralError::syntax(&input, &exact).expect("exact source");
    let below_source =
        crate::app::OwnedNumericLiteralError::syntax(&input, &below).expect("below source");

    // Act
    let exact_frame = PgWireError::controlled_diagnostic(
        super::PgWireSeverity::Error,
        exact_source.sqlstate(),
        exact_source.message(),
        &exact,
    );
    let below_frame = PgWireError::controlled_diagnostic(
        super::PgWireSeverity::Error,
        below_source.sqlstate(),
        below_source.message(),
        &below,
    );

    // Assert
    assert!(exact_frame.is_ok());
    assert!(matches!(below_frame, Err(CassieError::ResourceLimit(_))));
    drop(exact_frame);
    drop(exact_source);
    drop(below_source);
    assert_eq!(exact.current_query_memory_bytes(), 0);
    assert_eq!(below.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_numeric_error_frame_on_interruption_during_message_or_frame_copy() {
    // Arrange
    use super::parameter_semantic::{admitted_error_frame_observing, FrameCopyPhase};
    let message = format!(
        "invalid input syntax for type numeric: \"{}\"",
        "a".repeat(4096)
    );
    for phase in [FrameCopyPhase::Message, FrameCopyPhase::Frame] {
        let cancellation = QueryCancellationHandle::new();
        let interrupted_controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            cancellation.clone(),
        );
        let memory = interrupted_controls
            .reserve_query_memory(std::mem::size_of::<PgWireError>() + message.len())
            .expect("message admission");
        let error = PgWireError::new(super::PgWireSeverity::Fatal, "22P02", message.clone());
        let mut observed = false;

        // Act
        let result =
            admitted_error_frame_observing(error, &interrupted_controls, memory, |current| {
                if current == phase && !observed {
                    observed = true;
                    cancellation.cancel();
                }
            });

        // Assert
        assert!(observed);
        assert!(matches!(result, Err(CassieError::QueryCancelled)));
        assert_eq!(interrupted_controls.current_query_memory_bytes(), 0);
        let recovery_controls = controls(65_536);
        let recovery = PgWireError::controlled_diagnostic(
            super::PgWireSeverity::Fatal,
            "22P02",
            &message,
            &recovery_controls,
        )
        .expect("subsequent frame");
        assert!(matches!(recovery.severity, super::PgWireSeverity::Fatal));
        assert_eq!(recovery.code, "22P02");
        drop(recovery);
        assert_eq!(recovery_controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_reject_copy_unknown_column_frame_above_backend_limit_before_admission() {
    // Arrange
    let limit = super::super::writers::MAX_BACKEND_FRAME_BYTES;
    let (_, empty_length, _) =
        super::parameter_semantic::copy_unknown_column_frame_sizes(0).expect("empty name");
    let max_name = limit - empty_length;
    // Act
    let at_limit =
        super::parameter_semantic::copy_unknown_column_frame_sizes(max_name).expect("at limit");
    let above_limit = super::parameter_semantic::copy_unknown_column_frame_sizes(max_name + 1);
    // Assert
    assert_eq!(at_limit.1, limit);
    assert!(
        matches!(above_limit, Err(CassieError::ResourceLimit(message)) if message == "pgwire backend frame exceeds 16777216 bytes")
    );
}
