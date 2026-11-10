use std::time::Instant;

use crate::runtime::QueryCancellationHandle;
use tokio::io::AsyncWriteExt;

use super::{CopyReadOutcome, FrontendMessage};

async fn read_test(
    cancellation: Option<&QueryCancellationHandle>,
    deadline: Option<Instant>,
    reader: &mut (impl tokio::io::AsyncRead + Unpin),
) -> CopyReadOutcome {
    let controls = crate::runtime::QueryExecutionControls::from_limits(
        &crate::config::CassieRuntimeLimits::default(),
        Instant::now(),
    );
    super::read_admitted_copy_message(cancellation, deadline, &controls, reader).await
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

#[test]
fn should_report_idle_copy_deadline() {
    // Arrange
    let runtime = runtime();
    runtime.block_on(async {
        let (_writer, mut reader) = tokio::io::duplex(64);
        // Act
        let outcome = read_test(None, Some(Instant::now()), &mut reader).await;
        // Assert
        assert!(matches!(outcome, CopyReadOutcome::DeadlineIdle));
    });
}

async fn deadline_after_prefix(prefix: &[u8]) -> CopyReadOutcome {
    let (mut writer, mut reader) = tokio::io::duplex(64);
    writer.write_all(prefix).await.expect("write frame prefix");
    read_test(None, Some(Instant::now()), &mut reader).await
}

#[test]
fn should_close_copy_on_deadline_during_partial_length() {
    // Arrange
    let runtime = runtime();
    // Act
    let outcome = runtime.block_on(deadline_after_prefix(&[b'd', 0, 0]));
    // Assert
    assert!(matches!(outcome, CopyReadOutcome::DeadlinePartial));
}

#[test]
fn should_close_copy_on_deadline_during_partial_body() {
    // Arrange
    let runtime = runtime();
    // Act
    let outcome = runtime.block_on(deadline_after_prefix(&[b'd', 0, 0, 0, 8, b'x']));
    // Assert
    assert!(matches!(outcome, CopyReadOutcome::DeadlinePartial));
}

#[test]
fn should_prefer_completed_copy_frame_when_deadline_is_ready() {
    // Arrange
    let runtime = runtime();
    // Act
    let outcome = runtime.block_on(deadline_after_prefix(&[b'c', 0, 0, 0, 4]));
    // Assert
    assert!(matches!(
        outcome,
        CopyReadOutcome::AdmittedMessage(Ok((FrontendMessage::CopyDone, _)))
    ));
}

#[test]
fn should_prefer_cancel_request_over_idle_copy_deadline() {
    // Arrange
    let runtime = runtime();
    runtime.block_on(async {
        let (_writer, mut reader) = tokio::io::duplex(64);
        let cancellation = QueryCancellationHandle::new();
        cancellation.cancel();
        // Act
        let outcome = read_test(Some(&cancellation), Some(Instant::now()), &mut reader).await;
        // Assert
        assert!(matches!(outcome, CopyReadOutcome::Cancelled));
    });
}

#[test]
fn should_read_copy_frame_with_timeout_disabled() {
    // Arrange
    let runtime = runtime();
    runtime.block_on(async {
        let (mut writer, mut reader) = tokio::io::duplex(64);
        writer
            .write_all(&[b'c', 0, 0, 0, 4])
            .await
            .expect("write frame");
        // Act
        let outcome = read_test(None, None, &mut reader).await;
        // Assert
        assert!(matches!(
            outcome,
            CopyReadOutcome::AdmittedMessage(Ok((FrontendMessage::CopyDone, _)))
        ));
    });
}

struct ObserveTag<R> {
    reader: R,
    tag_read: Option<tokio::sync::oneshot::Sender<()>>,
}

impl<R: tokio::io::AsyncRead + Unpin> tokio::io::AsyncRead for ObserveTag<R> {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        context: &mut std::task::Context<'_>,
        buffer: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let before = buffer.filled().len();
        let result = std::pin::Pin::new(&mut self.reader).poll_read(context, buffer);
        if buffer.filled().len() > before {
            if let Some(tag_read) = self.tag_read.take() {
                let _ = tag_read.send(());
            }
        }
        result
    }
}

#[test]
fn should_close_partial_copy_frame_when_cancel_and_deadline_overlap() {
    // Arrange
    let runtime = runtime();
    runtime.block_on(async {
        let (mut writer, reader) = tokio::io::duplex(64);
        let (tag_read, observed) = tokio::sync::oneshot::channel();
        let mut reader = ObserveTag {
            reader,
            tag_read: Some(tag_read),
        };
        let cancellation = QueryCancellationHandle::new();
        let reader_cancellation = cancellation.clone();
        let deadline = Instant::now() + std::time::Duration::from_secs(1);
        let read = tokio::spawn(async move {
            read_test(Some(&reader_cancellation), Some(deadline), &mut reader).await
        });
        writer.write_all(b"d").await.expect("send tag");
        observed.await.expect("reader consumed tag");
        // Act
        cancellation.cancel();
        let outcome = read.await.expect("reader task");
        // Assert
        assert!(matches!(outcome, CopyReadOutcome::DeadlinePartial));
    });
}

#[test]
fn should_drain_denied_frame_and_preserve_the_following_copy_frame() {
    // Arrange
    let runtime = runtime();
    runtime.block_on(async {
        let limits = crate::config::CassieRuntimeLimits {
            query_memory_budget_bytes: 4,
            ..crate::config::CassieRuntimeLimits::default()
        };
        let controls = crate::runtime::QueryExecutionControls::from_limits(&limits, Instant::now());
        let owner = controls.reserve_query_memory(4).expect("owner admission");
        let (mut writer, mut reader) = tokio::io::duplex(64);
        writer
            .write_all(&[b'd', 0, 0, 0, 5, b'x', b'c', 0, 0, 0, 4])
            .await
            .expect("two frames");
        // Act
        let denied = super::read_admitted_copy_message(None, None, &controls, &mut reader).await;
        let next = super::read_admitted_copy_message(None, None, &controls, &mut reader).await;
        // Assert
        assert!(matches!(denied, CopyReadOutcome::Resource(_)));
        assert!(matches!(
            next,
            CopyReadOutcome::AdmittedMessage(Ok((FrontendMessage::CopyDone, _)))
        ));
        assert_eq!(controls.current_query_memory_bytes(), 4);
        drop(owner);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    });
}

#[test]
fn should_close_if_copy_deadline_interrupts_a_denied_frame_drain() {
    // Arrange
    let runtime = runtime();
    runtime.block_on(async {
        let limits = crate::config::CassieRuntimeLimits {
            query_memory_budget_bytes: 0,
            ..crate::config::CassieRuntimeLimits::default()
        };
        let controls = crate::runtime::QueryExecutionControls::from_limits(&limits, Instant::now());
        let (mut writer, mut reader) = tokio::io::duplex(64);
        writer
            .write_all(&[b'd', 0, 0, 0, 6, b'x'])
            .await
            .expect("partial denied frame");
        // Act
        let outcome =
            super::read_admitted_copy_message(None, Some(Instant::now()), &controls, &mut reader)
                .await;
        // Assert
        assert!(matches!(outcome, CopyReadOutcome::DeadlinePartial));
        assert_eq!(controls.current_query_memory_bytes(), 0);
    });
}

#[test]
fn should_reject_malformed_copy_frame_length_without_leaking_admission() {
    // Arrange
    let runtime = runtime();
    runtime.block_on(async {
        let controls = crate::runtime::QueryExecutionControls::from_limits(
            &crate::config::CassieRuntimeLimits::default(),
            Instant::now(),
        );
        let (mut writer, mut reader) = tokio::io::duplex(64);
        writer
            .write_all(&[b'd', 0, 0, 0, 3])
            .await
            .expect("malformed length");
        // Act
        let outcome = super::read_admitted_copy_message(None, None, &controls, &mut reader).await;
        // Assert
        assert!(matches!(outcome, CopyReadOutcome::FramingInvalid));
        assert_eq!(controls.current_query_memory_bytes(), 0);
    });
}

#[test]
fn should_reject_unexpected_copy_frame_without_decoding_and_keep_alignment() {
    // Arrange
    let runtime = runtime();
    runtime.block_on(async {
        let controls = crate::runtime::QueryExecutionControls::from_limits(
            &crate::config::CassieRuntimeLimits::default(),
            Instant::now(),
        );
        let (mut writer, mut reader) = tokio::io::duplex(64);
        writer
            .write_all(&[b'P', 0, 0, 0, 5, b'x', b'c', 0, 0, 0, 4])
            .await
            .expect("frames");
        // Act
        let first = super::read_admitted_copy_message(None, None, &controls, &mut reader).await;
        let second = super::read_admitted_copy_message(None, None, &controls, &mut reader).await;
        // Assert
        assert!(matches!(
            first,
            CopyReadOutcome::AdmittedMessage(Ok((FrontendMessage::Unknown, _)))
        ));
        assert!(matches!(
            second,
            CopyReadOutcome::AdmittedMessage(Ok((FrontendMessage::CopyDone, _)))
        ));
    });
}

#[test]
fn should_charge_copy_fail_decode_until_frame_owner_is_released() {
    // Arrange
    let runtime = runtime();
    runtime.block_on(async {
        let controls = crate::runtime::QueryExecutionControls::from_limits(
            &crate::config::CassieRuntimeLimits::default(),
            Instant::now(),
        );
        let (mut writer, mut reader) = tokio::io::duplex(64);
        writer
            .write_all(&[b'f', 0, 0, 0, 6, b'x', 0])
            .await
            .expect("copy fail");
        // Act
        let frame = super::read_admitted_copy_message(None, None, &controls, &mut reader).await;
        // Assert
        assert_eq!(controls.current_query_memory_bytes(), 4);
        assert!(matches!(
            &frame,
            CopyReadOutcome::AdmittedMessage(Ok((FrontendMessage::CopyFail(_), _)))
        ));
        drop(frame);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    });
}

#[test]
fn should_reject_copy_fail_with_missing_null_or_invalid_utf8() {
    // Arrange
    let runtime = runtime();
    runtime.block_on(async {
        for payload in [b"x".as_slice(), &[0xff, 0][..]] {
            let controls = crate::runtime::QueryExecutionControls::from_limits(
                &crate::config::CassieRuntimeLimits::default(),
                Instant::now(),
            );
            let (mut writer, mut reader) = tokio::io::duplex(64);
            writer.write_all(b"f").await.expect("tag");
            writer
                .write_all(&(i32::try_from(payload.len() + 4).expect("length")).to_be_bytes())
                .await
                .expect("length");
            writer.write_all(payload).await.expect("payload");
            // Act
            let outcome =
                super::read_admitted_copy_message(None, None, &controls, &mut reader).await;
            // Assert
            assert!(matches!(
                outcome,
                CopyReadOutcome::AdmittedMessage(Err(super::HandshakeError::Invalid(_)))
            ));
            assert_eq!(controls.current_query_memory_bytes(), 0);
        }
    });
}

#[test]
fn should_report_invalid_utf8_before_trailing_bytes_in_copy_fail() {
    // Arrange
    let runtime = runtime();
    runtime.block_on(async {
        let controls = crate::runtime::QueryExecutionControls::from_limits(
            &crate::config::CassieRuntimeLimits::default(),
            Instant::now(),
        );
        let (mut writer, mut reader) = tokio::io::duplex(64);
        writer
            .write_all(&[b'f', 0, 0, 0, 7, 0xff, 0, b'x'])
            .await
            .expect("frame");
        // Act
        let outcome = super::read_admitted_copy_message(None, None, &controls, &mut reader).await;
        // Assert
        match outcome {
            CopyReadOutcome::AdmittedMessage(Err(super::HandshakeError::Invalid(message))) => {
                assert_eq!(message, "invalid UTF-8 in startup option");
            }
            _ => panic!("expected invalid UTF-8"),
        }
    });
}

#[test]
fn should_finish_denied_copy_frame_before_reporting_cancellation() {
    // Arrange
    let runtime = runtime();
    runtime.block_on(async {
        let limits = crate::config::CassieRuntimeLimits {
            query_memory_budget_bytes: 0,
            ..crate::config::CassieRuntimeLimits::default()
        };
        let controls = crate::runtime::QueryExecutionControls::from_limits(&limits, Instant::now());
        let cancellation = QueryCancellationHandle::new();
        let reader_cancellation = cancellation.clone();
        let (mut writer, reader) = tokio::io::duplex(64);
        let (tag_read, observed) = tokio::sync::oneshot::channel();
        let mut reader = ObserveTag {
            reader,
            tag_read: Some(tag_read),
        };
        let read = tokio::spawn(async move {
            super::read_admitted_copy_message(
                Some(&reader_cancellation),
                None,
                &controls,
                &mut reader,
            )
            .await
        });
        writer
            .write_all(&[b'd', 0, 0, 0, 6, b'x'])
            .await
            .expect("partial frame");
        observed.await.expect("frame tag consumed");
        // Act
        cancellation.cancel();
        writer.write_all(b"y").await.expect("finish frame");
        let outcome = read.await.expect("reader task");
        // Assert
        assert!(matches!(outcome, CopyReadOutcome::Cancelled));
    });
}
