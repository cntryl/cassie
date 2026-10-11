use std::future::Future;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};
use std::time::Instant;

use tokio::io::AsyncWrite;

use super::{
    take_portal_execution, write_materialized_result, write_suspended_result,
    MaterializedPortalWriteRequest, Portal, PortalExecution, QueryResult, SessionState,
    SuspendedPortalWriteRequest,
};
use crate::config::CassieRuntimeLimits;
use crate::executor::ColumnMeta;
use crate::runtime::QueryExecutionControls;
use crate::types::Value;

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime")
}

fn result(controls: &QueryExecutionControls) -> QueryResult {
    let mut builder = QueryResult::try_builder(controls).expect("result builder");
    builder
        .try_push_column(&ColumnMeta::text("payload"))
        .expect("column");
    for value in ["first", "second", "third"] {
        builder
            .try_push_row(&[Value::String(value.repeat(128))])
            .expect("admitted row");
    }
    builder.try_set_command("SELECT").expect("command");
    builder.finish().expect("owned result")
}

fn state(limits: &CassieRuntimeLimits) -> (SessionState, PortalExecution) {
    let mut state = SessionState::new(limits);
    state.portals.insert(
        "retained".to_owned(),
        Portal {
            name: "retained".to_owned(),
            statement_name: "source".to_owned(),
            prepared_id: 1,
            params: Vec::new(),
            result_formats: Vec::new(),
            described: false,
            suspended: None,
            completed_command: None,
        },
    );
    let portal = PortalExecution::from(state.portals.get("retained").expect("portal"));
    (state, portal)
}

async fn suspend(
    writer: &mut (impl AsyncWrite + Unpin),
    state: &mut SessionState,
    portal: &PortalExecution,
    result: QueryResult,
) {
    write_materialized_result(
        writer,
        MaterializedPortalWriteRequest {
            state,
            portal_name: "retained",
            portal,
            result,
            cancellation: None,
            max_rows: 1,
            result_cap: 100,
            rows_emitted: 0,
            row_description_sent: false,
        },
    )
    .await
    .expect("suspend first page");
}

#[test]
fn should_keep_complete_payload_admitted_while_portal_offsets_advance() {
    // Arrange
    let limits = CassieRuntimeLimits::default();
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let result = result(&controls);
    let retained = controls.current_query_memory_bytes();
    let (mut state, portal) = state(&limits);
    let mut writer = tokio::io::sink();

    // Act
    let (first_offset, second_offset, portal_bytes) = runtime().block_on(async {
        suspend(&mut writer, &mut state, &portal, result).await;
        let first_offset = state.portals["retained"]
            .suspended
            .as_ref()
            .expect("first page")
            .row_offset;
        let portal_bytes = state.portal_memory_controls.current_query_memory_bytes();
        let (portal, suspended) = take_portal_execution(&mut state, "retained").expect("resume");
        write_suspended_result(
            &mut writer,
            SuspendedPortalWriteRequest {
                state: &mut state,
                portal_name: "retained",
                portal: &portal,
                suspended: suspended.expect("retained owner"),
                max_rows: 1,
                result_cap: 100,
            },
        )
        .await
        .expect("second page");
        let second_offset = state.portals["retained"]
            .suspended
            .as_ref()
            .expect("second page")
            .row_offset;
        (first_offset, second_offset, portal_bytes)
    });
    let producer_during_resume = controls.current_query_memory_bytes();
    let portal_during_resume = state.portal_memory_controls.current_query_memory_bytes();
    drop(state.remove_portal("retained"));

    // Assert
    assert_eq!((first_offset, second_offset), (1, 2));
    assert!(retained > 1_024);
    assert_eq!(producer_during_resume, retained);
    assert!(portal_bytes >= retained);
    assert_eq!(portal_during_resume, portal_bytes);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    assert_eq!(state.portal_memory_controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_complete_payload_when_materialized_portal_finishes() {
    // Arrange
    let limits = CassieRuntimeLimits::default();
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let result = result(&controls);
    let (mut state, portal) = state(&limits);
    let mut writer = tokio::io::sink();

    // Act
    runtime().block_on(async {
        suspend(&mut writer, &mut state, &portal, result).await;
        let (portal, suspended) = take_portal_execution(&mut state, "retained").expect("resume");
        write_suspended_result(
            &mut writer,
            SuspendedPortalWriteRequest {
                state: &mut state,
                portal_name: "retained",
                portal: &portal,
                suspended: suspended.expect("retained owner"),
                max_rows: 0,
                result_cap: 100,
            },
        )
        .await
        .expect("remaining pages");
    });

    // Assert
    assert!(state.portals["retained"].suspended.is_none());
    assert_eq!(controls.current_query_memory_bytes(), 0);
    assert_eq!(state.portal_memory_controls.current_query_memory_bytes(), 0);
}

struct PendingWrite;

impl AsyncWrite for PendingWrite {
    fn poll_write(self: Pin<&mut Self>, _: &mut Context<'_>, _: &[u8]) -> Poll<io::Result<usize>> {
        Poll::Pending
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Pending
    }

    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

#[test]
fn should_keep_result_admitted_until_a_pending_wire_write_is_dropped() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let result = result(&controls);
    let retained = controls.current_query_memory_bytes();
    let mut writer = PendingWrite;
    let mut write = Box::pin(
        crate::pgwire::connection::writers::write_simple_query_result(&mut writer, result),
    );
    let mut context = Context::from_waker(Waker::noop());

    // Act
    let pending = write.as_mut().poll(&mut context).is_pending();
    let during_write = controls.current_query_memory_bytes();
    drop(write);

    // Assert
    assert!(pending);
    assert!(
        during_write > retained,
        "pending frames keep their encoding scratch admitted"
    );
    assert!(retained > 1_024);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[derive(Default)]
struct FailingWrite {
    calls: usize,
}

impl AsyncWrite for FailingWrite {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
        _: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.calls += 1;
        Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()))
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

#[test]
fn should_release_result_and_frame_admission_after_a_wire_write_fails() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let result = result(&controls);
    let mut writer = FailingWrite::default();

    // Act
    let error = runtime().block_on(
        crate::pgwire::connection::writers::write_simple_query_result(&mut writer, result),
    );

    // Assert
    assert_eq!(
        error.expect_err("write failure").kind(),
        io::ErrorKind::BrokenPipe
    );
    assert_eq!(writer.calls, 1);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_frame_scratch_before_writing_any_result_bytes() {
    // Arrange
    let limits = CassieRuntimeLimits::default();
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let result = result(&controls);
    let retained = controls.current_query_memory_bytes();
    let occupied = controls
        .reserve_query_memory(limits.query_memory_budget_bytes - retained)
        .expect("fill remaining budget");
    let mut writer = FailingWrite::default();

    // Act
    let error = runtime().block_on(
        crate::pgwire::connection::writers::write_simple_query_result(&mut writer, result),
    );
    let during_fill = controls.current_query_memory_bytes();
    drop(occupied);

    // Assert
    let error = error.expect_err("frame admission must fail before writing");
    assert!(matches!(
        crate::pgwire::connection::writers::query_memory_error(&error),
        Some(crate::app::CassieError::ResourceLimit(_))
    ));
    assert_eq!(writer.calls, 0);
    assert_eq!(during_fill, limits.query_memory_budget_bytes - retained);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
