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
