use super::{parameter_semantic, PgWireError, PgWireSeverity};
use crate::app::CassieError;
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use std::time::Instant;

#[derive(Debug)]
enum PrivateExecutionError {
    Cassie(CassieError),
    Row(crate::runtime::accounted::json_arena::JsonbDiagnostic),
}

impl PrivateExecutionError {
    fn frame(
        &self,
        severity: PgWireSeverity,
        controls: &QueryExecutionControls,
    ) -> Result<PgWireError, CassieError> {
        match self {
            Self::Cassie(error) => Ok(PgWireError::from_cassie_error(severity, error)),
            Self::Row(diagnostic) => {
                PgWireError::controlled_diagnostic(severity, "42601", diagnostic.as_str(), controls)
            }
        }
    }
}

fn simulated_execution(controls: &QueryExecutionControls) -> Result<(), PrivateExecutionError> {
    match crate::midge::row_blob::tests::owned_row_diagnostic(controls) {
        Ok(diagnostic) => Err(PrivateExecutionError::Row(diagnostic)),
        Err(error) => Err(PrivateExecutionError::Cassie(error)),
    }
}

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

#[test]
fn should_keep_worker_row_diagnostic_and_admitted_pgwire_frame_independent() {
    // Arrange
    let controls = controls(1_048_576);
    let worker_controls = controls.clone();
    let worker = std::thread::spawn(move || simulated_execution(&worker_controls));
    let error = worker.join().expect("worker").expect_err("row error");
    let expected = serde_json::from_slice::<serde_json::Value>(b"[}\xff ")
        .expect_err("legacy")
        .to_string();
    let message = format!("invalid JSON in row blob: {expected}");
    // Act
    let frame = error
        .frame(PgWireSeverity::Error, &controls)
        .expect("frame");
    let before = controls.current_query_memory_bytes();
    drop(error);
    // Assert
    assert_eq!(frame.code, "42601");
    assert_eq!(frame.message, message);
    assert!(frame
        .encoded_semantic_frame()
        .expect("encoded")
        .windows(7)
        .any(|field| field == b"C42601\0"));
    assert!(controls.current_query_memory_bytes() > 0);
    assert!(controls.current_query_memory_bytes() < before);
    drop(frame);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_worker_cancellation_in_private_execution_error() {
    // Arrange
    let cancel = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel.clone(),
    );
    cancel.cancel();
    // Act
    let result = simulated_execution(&controls);
    // Assert
    assert!(matches!(
        result,
        Err(PrivateExecutionError::Cassie(CassieError::QueryCancelled))
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_fatal_severity_for_owned_row_diagnostic() {
    // Arrange
    let controls = controls(1_048_576);
    let diagnostic =
        crate::midge::row_blob::tests::owned_row_diagnostic(&controls).expect("diagnostic");
    // Act
    let frame = PgWireError::controlled_diagnostic(
        PgWireSeverity::Fatal,
        "42601",
        diagnostic.as_str(),
        &controls,
    )
    .expect("frame");
    // Assert
    assert!(matches!(frame.severity, PgWireSeverity::Fatal));
    assert!(frame
        .encoded_semantic_frame()
        .expect("encoded")
        .windows(7)
        .any(|field| field == b"SFATAL\0"));
    drop(frame);
    drop(diagnostic);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_enforce_measured_worker_diagnostic_and_frame_overlap_budget() {
    // Arrange
    let baseline = controls(1_048_576);
    let diagnostic =
        crate::midge::row_blob::tests::owned_row_diagnostic(&baseline).expect("diagnostic");
    let filler = baseline.reserve_query_memory(4096).expect("filler");
    let frame = PgWireError::controlled_diagnostic(
        PgWireSeverity::Error,
        "42601",
        diagnostic.as_str(),
        &baseline,
    )
    .expect("frame");
    let peak = baseline.peak_query_memory_bytes();
    drop(frame);
    drop(filler);
    drop(diagnostic);
    // Act / Assert
    for (budget, succeeds) in [(peak, true), (peak - 1, false)] {
        let controls = controls(budget);
        let diagnostic =
            crate::midge::row_blob::tests::owned_row_diagnostic(&controls).expect("diagnostic");
        let filler = controls.reserve_query_memory(4096).expect("filler");
        let frame = PgWireError::controlled_diagnostic(
            PgWireSeverity::Error,
            "42601",
            diagnostic.as_str(),
            &controls,
        );
        assert_eq!(frame.is_ok(), succeeds);
        drop(frame);
        drop(filler);
        drop(diagnostic);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_release_partial_frame_after_actual_copy_cancellation() {
    // Arrange
    use parameter_semantic::{admitted_error_frame_observing, FrameCopyPhase};
    let cancel = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel.clone(),
    );
    let diagnostic =
        crate::midge::row_blob::tests::owned_row_diagnostic(&controls).expect("diagnostic");
    let text = diagnostic.as_str();
    let memory = controls
        .reserve_query_memory(std::mem::size_of::<PgWireError>() + text.len())
        .expect("message reservation");
    let mut message = String::new();
    message
        .try_reserve_exact(text.len())
        .expect("message allocation");
    assert!(message.capacity() <= text.len());
    message.push_str(text);
    let error = PgWireError::new(PgWireSeverity::Error, "42601", message);
    let mut copied = 0;
    // Act
    let result = admitted_error_frame_observing(error, &controls, memory, |phase| {
        if matches!(phase, FrameCopyPhase::Frame) {
            copied += 1;
            cancel.cancel();
        }
    });
    // Assert
    assert_eq!(copied, 1);
    assert!(matches!(result, Err(CassieError::QueryCancelled)));
    assert!(controls.current_query_memory_bytes() > 0);
    drop(diagnostic);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
