use std::time::Instant;

use crate::app::CassieSession;
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryExecutionControls;

#[test]
fn should_close_copy_for_oversized_unknown_column_frame_without_writing() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let at_limit_session = CassieSession::new("root".into(), None);
    let above_limit_session = CassieSession::new("root".into(), None);
    at_limit_session.begin_transaction(None).expect("begin");
    above_limit_session.begin_transaction(None).expect("begin");
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let overhead =
        "unresolvable column reference '".len() + "'; known fields or aliases required".len() + 21;
    let max_name = super::super::writers::MAX_BACKEND_FRAME_BYTES - overhead;
    let mut writer = Vec::new();
    // Pass synthetic lengths to exercise the boundary without a 16 MiB fixture.
    // Act
    let (at_limit, above_limit, after_first) = runtime.block_on(async {
        let at_limit = super::write_copy_unknown_column_with_length(
            &mut writer,
            &at_limit_session,
            "x",
            max_name,
            &controls,
        )
        .await;
        let after_first = writer.len();
        let above_limit = super::write_copy_unknown_column_with_length(
            &mut writer,
            &above_limit_session,
            "x",
            max_name + 1,
            &controls,
        )
        .await;
        (at_limit, above_limit, after_first)
    });
    // Assert
    assert!(at_limit);
    assert!(!above_limit);
    assert!(after_first > 0);
    assert_eq!(writer.len(), after_first);
    assert_eq!(at_limit_session.transaction_status(), "failed");
    assert_eq!(above_limit_session.transaction_status(), "failed");
}

#[test]
fn should_close_copy_for_oversized_jsonb_diagnostic_without_writing() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let at_limit = CassieSession::new("root".into(), None);
    let above_limit = CassieSession::new("root".into(), None);
    at_limit.begin_transaction(None).expect("begin");
    above_limit.begin_transaction(None).expect("begin");
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let maximum = super::super::writers::MAX_BACKEND_FRAME_BYTES - 21;
    let mut writer = Vec::new();
    // Act
    let (first, second, after_first) = runtime.block_on(async {
        let first = super::write_copy_jsonb_diagnostic_with_length(
            &mut writer,
            &at_limit,
            "x",
            maximum,
            &controls,
        )
        .await;
        let after_first = writer.len();
        let second = super::write_copy_jsonb_diagnostic_with_length(
            &mut writer,
            &above_limit,
            "x",
            maximum + 1,
            &controls,
        )
        .await;
        (first, second, after_first)
    });
    // Assert
    assert!(first);
    assert!(!second);
    assert!(after_first > 0);
    assert_eq!(writer.len(), after_first);
    assert_eq!(at_limit.transaction_status(), "failed");
    assert_eq!(above_limit.transaction_status(), "failed");
}

#[test]
fn should_report_jsonb_frame_admission_with_cancellation_failures_with_fail_transaction() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let denied_session = CassieSession::new("root".into(), None);
    let canceled_session = CassieSession::new("root".into(), None);
    denied_session.begin_transaction(None).expect("begin");
    canceled_session.begin_transaction(None).expect("begin");
    let denied = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: 0,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    let cancellation = crate::runtime::QueryCancellationHandle::new();
    cancellation.cancel();
    let canceled = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation,
    );
    let mut denied_bytes = Vec::new();
    let mut canceled_bytes = Vec::new();
    // Act
    let (denied_result, canceled_result) = runtime.block_on(async {
        let first = super::write_copy_jsonb_diagnostic_with_length(
            &mut denied_bytes,
            &denied_session,
            "jsonb",
            5,
            &denied,
        )
        .await;
        let second = super::write_copy_jsonb_diagnostic_with_length(
            &mut canceled_bytes,
            &canceled_session,
            "jsonb",
            5,
            &canceled,
        )
        .await;
        (first, second)
    });
    // Assert
    assert!(denied_result && canceled_result);
    assert!(denied_bytes.windows(7).any(|bytes| bytes == b"C54000\0"));
    assert!(canceled_bytes.windows(7).any(|bytes| bytes == b"C57014\0"));
    assert_eq!(denied_session.transaction_status(), "failed");
    assert_eq!(canceled_session.transaction_status(), "failed");
    assert_eq!(denied.current_query_memory_bytes(), 0);
    assert_eq!(canceled.current_query_memory_bytes(), 0);
}

#[cfg(debug_assertions)]
#[test]
fn should_preserve_retryable_worker_failure_with_jsonb_copy_error_type() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let cassie =
        std::sync::Arc::new(
            crate::app::Cassie::new_in_memory_with_config(
                crate::config::CassieRuntimeConfig::default(),
            )
            .expect("Cassie"),
        );
    super::super::blocking::arm_next_retryable_failure_for_test(&cassie, "synthetic failure");
    // Act
    let result: Result<(), super::filter::CopyWhereError> =
        runtime.block_on(super::super::blocking::run_pgwire_blocking_with_error(
            cassie,
            "pgwire_copy_from_stdin",
            |_| panic!("worker should not run"),
        ));
    // Assert
    assert!(matches!(
        result,
        Err(super::filter::CopyWhereError::Cassie(
            crate::app::CassieError::StorageRetryable(_)
        ))
    ));
}
