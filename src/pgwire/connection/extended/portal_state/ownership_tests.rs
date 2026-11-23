use std::future::Future;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};
use std::time::Instant;

use tokio::io::AsyncWrite;

use super::{
    MaterializedPortalWriteRequest, Portal, PortalExecution, QueryResult, SessionState,
    SuspendedPortalWriteRequest, take_portal_execution, write_materialized_result,
    write_suspended_result,
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
            parameter_memory: Vec::new(),
            result_formats: Vec::new(),
            described: false,
            suspended: None,
            completed_command: None,
        },
    );
    let portal = PortalExecution::from(state.portals.get("retained").expect("portal"));
    (state, portal)
}

fn active_query_fixture(
    sql: &str,
) -> (
    std::sync::Arc<crate::app::Cassie>,
    crate::app::CassieSession,
    SessionState,
    crate::pgwire::protocol::PreparedStatement,
    PortalExecution,
) {
    let cassie =
        std::sync::Arc::new(
            crate::app::Cassie::new_in_memory_with_config(
                crate::config::CassieRuntimeConfig::default(),
            )
            .expect("memory fixture"),
        );
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(&session, "CREATE TABLE t (id INT)", Vec::new())
        .expect("table");
    cassie
        .execute_sql(&session, "INSERT INTO t VALUES (1), (2), (3)", Vec::new())
        .expect("rows");
    let mut state = SessionState::new(&cassie.runtime.limits());
    state.backend_registration = Some(cassie.runtime.register_pgwire_backend());
    let parsed = crate::sql::parse_statement(sql).expect("statement");
    let prepared = crate::pgwire::protocol::PreparedStatement {
        id: 1,
        name: "source".to_owned(),
        query: sql.to_owned(),
        sql_fingerprint: crate::runtime::sql_fingerprint(&parsed),
        parsed: Some(parsed),
        parameter_count: 0,
        parameter_types: Vec::new(),
        described: true,
    };
    state.portals.insert(
        "retained".to_owned(),
        Portal {
            name: "retained".to_owned(),
            statement_name: "source".to_owned(),
            prepared_id: 1,
            params: Vec::new(),
            parameter_memory: Vec::new(),
            result_formats: Vec::new(),
            described: true,
            suspended: None,
            completed_command: None,
        },
    );
    let portal = PortalExecution::from(state.portals.get("retained").expect("portal"));
    (cassie, session, state, prepared, portal)
}

#[test]
fn should_retain_json_bind_memory_while_portal_execution_clones_parameters() {
    // Arrange
    let limits = CassieRuntimeLimits::default();
    let mut state = SessionState::new(&limits);
    let parsed = crate::runtime::accounted::json_arena::parse_value(
        br#"{"$serde_json::private::Number":"ordinary"}"#,
        &state.portal_memory_controls,
    )
    .expect("parse JSON parameter");
    let (value, memory) = parsed.into_parts();
    state.portals.insert(
        "json".to_string(),
        Portal {
            name: "json".to_string(),
            statement_name: "query".to_string(),
            prepared_id: 1,
            params: vec![Value::Json(value)],
            parameter_memory: vec![memory],
            result_formats: Vec::new(),
            described: false,
            suspended: None,
            completed_command: None,
        },
    );
    assert!(state.portal_memory_controls.current_query_memory_bytes() > 0);

    // Act
    let execution = PortalExecution::from(state.portals.get("json").expect("portal"));
    drop(execution);
    let (execution, _) = take_portal_execution(&mut state, "json").expect("execution");
    drop(execution);
    let retained_bytes = state.portal_memory_controls.current_query_memory_bytes();
    drop(state.remove_portal("json"));

    // Assert
    assert!(retained_bytes > 0);
    assert_eq!(state.portal_memory_controls.current_query_memory_bytes(), 0);
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
                active_cancellation: None,
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
                active_cancellation: None,
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

struct CancelAtFlushWrite {
    runtime: std::sync::Arc<crate::runtime::RuntimeState>,
    process_id: i32,
    secret_key: i32,
    attempted: bool,
    canceled: bool,
    frame_tags: Vec<u8>,
}

impl AsyncWrite for CancelAtFlushWrite {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.frame_tags
            .push(*bytes.first().expect("portal frame has a tag"));
        Poll::Ready(Ok(bytes.len()))
    }

    fn poll_flush(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        if !self.attempted {
            self.attempted = true;
            let runtime = std::sync::Arc::clone(&self.runtime);
            self.canceled = runtime.cancel_pgwire_backend(self.process_id, self.secret_key);
        }
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

#[test]
fn should_release_output_admission_after_a_wire_write_fails() {
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
fn should_ignore_cancellation_from_a_prior_portal_execution_when_resuming() {
    // Arrange
    let limits = CassieRuntimeLimits::default();
    let producer = QueryExecutionControls::from_limits(&limits, Instant::now());
    let (mut state, portal) = state(&limits);
    let mut writer = tokio::io::sink();
    runtime().block_on(suspend(&mut writer, &mut state, &portal, result(&producer)));
    let (portal, suspended) = take_portal_execution(&mut state, "retained").expect("resume");
    let mut suspended = suspended.expect("suspended result and frames");
    let stale_cancellation = crate::runtime::QueryCancellationHandle::new();
    stale_cancellation.cancel();
    suspended.cancellation = Some(stale_cancellation);
    let mut config = crate::config::CassieRuntimeConfig::from_env().expect("configuration");
    config.password = "portal-cancellation-control".to_owned();
    let cassie = std::sync::Arc::new(
        crate::app::Cassie::new_in_memory_with_config(config).expect("memory fixture"),
    );
    state.backend_registration = Some(cassie.runtime.register_pgwire_backend());
    let session = cassie.create_session("tester", None);

    // Act
    let resumed = runtime().block_on(super::super::portal_streaming::resume_suspended_portal(
        std::sync::Arc::clone(&cassie),
        &mut writer,
        super::super::portal_streaming::SuspendedPortalRequest {
            state: &mut state,
            session: &session,
            portal_name: "retained",
            portal: &portal,
            suspended,
            max_rows: 1,
        },
    ));

    // Assert
    assert!(
        resumed.is_ok(),
        "a prior generation must not cancel this Execute"
    );
    assert!(state.portals["retained"].suspended.is_some());
    drop(state.remove_portal("retained"));
    assert_eq!(state.portal_memory_controls.current_query_memory_bytes(), 0);
    assert_eq!(producer.current_query_memory_bytes(), 0);
}

#[test]
fn should_cancel_an_active_materialized_portal_page_at_a_frame_boundary() {
    // Arrange
    let limits = CassieRuntimeLimits::default();
    let producer = QueryExecutionControls::from_limits(&limits, Instant::now());
    let (mut state, portal) = state(&limits);
    runtime().block_on(suspend(
        &mut tokio::io::sink(),
        &mut state,
        &portal,
        result(&producer),
    ));
    let cassie = {
        let mut config = crate::config::CassieRuntimeConfig::from_env().expect("configuration");
        config.password = "active-materialized-cancel".to_owned();
        let cassie = std::sync::Arc::new(
            crate::app::Cassie::new_in_memory_with_config(config).expect("memory fixture"),
        );
        let registration = cassie.runtime.register_pgwire_backend();
        let previous = registration.begin_query().suspend();
        state.backend_registration = Some(registration);
        state
            .portals
            .get_mut("retained")
            .expect("portal")
            .suspended
            .as_mut()
            .expect("suspended portal")
            .cancellation = Some(previous);
        cassie
    };
    let backend = state
        .backend_registration
        .as_ref()
        .expect("backend registration");
    let process_id = backend.process_id();
    let secret_key = backend.secret_key();
    let session = cassie.create_session("tester", None);
    let mut writer = CancelAtFlushWrite {
        runtime: std::sync::Arc::clone(&cassie.runtime),
        process_id,
        secret_key,
        attempted: false,
        canceled: false,
        frame_tags: Vec::new(),
    };
    let (portal, suspended) = take_portal_execution(&mut state, "retained").expect("resume");

    // Act
    let error = runtime().block_on(super::super::portal_streaming::resume_suspended_portal(
        std::sync::Arc::clone(&cassie),
        &mut writer,
        super::super::portal_streaming::SuspendedPortalRequest {
            state: &mut state,
            session: &session,
            portal_name: "retained",
            portal: &portal,
            suspended: suspended.expect("materialized result"),
            max_rows: 2,
        },
    ));

    // Assert
    assert!(
        writer.canceled,
        "resumed Execute is registered while writing"
    );
    assert_eq!(
        error.expect_err("active page cancellation").pg_error.code,
        "57014"
    );
    assert_eq!(writer.frame_tags, vec![b'D']);
    assert!(state.portals["retained"].suspended.is_none());
    assert_eq!(state.portal_memory_controls.current_query_memory_bytes(), 0);
    assert!(!cassie.runtime.cancel_pgwire_backend(process_id, secret_key));
}

#[test]
fn should_keep_cancellation_registered_while_streaming_portal_pages_are_written() {
    // Arrange
    for (sql, max_rows) in [
        ("SELECT id FROM t ORDER BY id", 1),
        ("SELECT id FROM t ORDER BY id", 3),
        (
            "WITH selected AS (SELECT id FROM t) SELECT id FROM selected ORDER BY id",
            1,
        ),
        (
            "WITH selected AS (SELECT id FROM t) SELECT id FROM selected ORDER BY id",
            3,
        ),
    ] {
        let (cassie, session, mut state, prepared, portal) = active_query_fixture(sql);
        let backend = state.backend_registration.as_ref().expect("backend");
        let (process_id, secret_key) = (backend.process_id(), backend.secret_key());
        let mut writer = CancelAtFlushWrite {
            runtime: std::sync::Arc::clone(&cassie.runtime),
            process_id,
            secret_key,
            attempted: false,
            canceled: false,
            frame_tags: Vec::new(),
        };

        // Act
        let error = runtime().block_on(super::super::execute_streaming_portal_page(
            std::sync::Arc::clone(&cassie),
            &mut writer,
            super::super::StreamingPortalPageRequest {
                state: &mut state,
                session: &session,
                portal_name: "retained",
                portal: &portal,
                prepared: &prepared,
                max_rows,
                rows_emitted: 0,
                cancellation: None,
            },
        ));

        // Assert
        assert!(writer.canceled, "active page cancellation must be accepted");
        assert_eq!(
            error.expect_err("active page cancellation").pg_error.code,
            "57014"
        );
        assert_eq!(writer.frame_tags, vec![b'D']);
        assert!(state.portals["retained"].suspended.is_none());
        assert!(!cassie.runtime.cancel_pgwire_backend(process_id, secret_key));
    }
}

#[test]
fn should_keep_cancellation_registered_while_prepared_portal_pages_are_written() {
    // Arrange
    for max_rows in [1, 3] {
        let sql = "SELECT id FROM t ORDER BY id";
        let (cassie, session, mut state, prepared, portal) = active_query_fixture(sql);
        let backend = state.backend_registration.as_ref().expect("backend");
        let (process_id, secret_key) = (backend.process_id(), backend.secret_key());
        let mut writer = CancelAtFlushWrite {
            runtime: std::sync::Arc::clone(&cassie.runtime),
            process_id,
            secret_key,
            attempted: false,
            canceled: false,
            frame_tags: Vec::new(),
        };

        // Act
        let error = runtime().block_on(super::super::execute_prepared_portal(
            std::sync::Arc::clone(&cassie),
            &cassie.runtime,
            &mut writer,
            &mut state,
            &session,
            super::super::PreparedPortalRequest {
                portal_name: "retained",
                portal: &portal,
                prepared: &prepared,
                parsed: prepared.parsed.clone().expect("parsed statement"),
                max_rows,
                standalone: false,
            },
        ));

        // Assert
        assert!(writer.canceled, "active page cancellation must be accepted");
        assert_eq!(
            error.expect_err("active page cancellation").pg_error.code,
            "57014"
        );
        assert_eq!(writer.frame_tags, vec![b'D']);
        assert!(state.portals["retained"].suspended.is_none());
        assert!(!cassie.runtime.cancel_pgwire_backend(process_id, secret_key));
    }
}

#[test]
fn should_acknowledge_success_after_a_transaction_publication() {
    // Arrange
    for sql in ["CREATE TABLE published (id INT)", "COMMIT"] {
        let (cassie, session, mut state, prepared, _) = active_query_fixture(sql);
        if sql == "COMMIT" {
            cassie
                .execute_sql(&session, "BEGIN", Vec::new())
                .expect("begin transaction");
            cassie
                .execute_sql(&session, "INSERT INTO t VALUES (4)", Vec::new())
                .expect("stage row");
        }
        state
            .prepared_statements
            .insert("source".to_owned(), prepared.clone());
        let backend = state.backend_registration.as_ref().expect("backend");
        let (process_id, secret_key) = (backend.process_id(), backend.secret_key());
        let mut writer = CancelAtFlushWrite {
            runtime: std::sync::Arc::clone(&cassie.runtime),
            process_id,
            secret_key,
            attempted: false,
            canceled: false,
            frame_tags: Vec::new(),
        };

        // Act
        let outcome = runtime().block_on(super::super::handle_execute(
            std::sync::Arc::clone(&cassie),
            &cassie.runtime,
            &mut writer,
            &mut state,
            &session,
            "retained".to_owned(),
            0,
        ));

        // Assert
        assert!(writer.attempted, "the writer issued CancelRequest");
        assert!(!writer.canceled, "published work is no longer cancelable");
        assert!(
            outcome.is_ok(),
            "successful publication must be acknowledged"
        );
        assert!(writer.frame_tags.contains(&b'C'));
        assert!(!session.is_transaction_active());
        assert!(!cassie.runtime.cancel_pgwire_backend(process_id, secret_key));
        if sql.starts_with("CREATE TABLE") {
            assert!(cassie.catalog.get_schema("published").is_some());
        } else {
            let rows = cassie
                .execute_sql(&session, "SELECT id FROM t", Vec::new())
                .expect("read committed row");
            assert_eq!(rows.row_count(), 4);
        }
    }
}

#[test]
fn should_release_retained_materialized_frames_when_a_resumed_wire_write_fails() {
    // Arrange
    let limits = CassieRuntimeLimits::default();
    let producer = QueryExecutionControls::from_limits(&limits, Instant::now());
    let (mut state, portal) = state(&limits);
    runtime().block_on(suspend(
        &mut tokio::io::sink(),
        &mut state,
        &portal,
        result(&producer),
    ));
    let (portal, suspended) = take_portal_execution(&mut state, "retained").expect("resume");
    let mut writer = FailingWrite::default();

    // Act
    let error = runtime().block_on(write_suspended_result(
        &mut writer,
        SuspendedPortalWriteRequest {
            state: &mut state,
            portal_name: "retained",
            portal: &portal,
            suspended: suspended.expect("suspended result and frames"),
            active_cancellation: None,
            max_rows: 1,
            result_cap: 100,
        },
    ));

    // Assert
    assert!(error.is_err());
    assert_eq!(writer.calls, 1);
    assert_eq!(state.portal_memory_controls.current_query_memory_bytes(), 0);
    assert_eq!(producer.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_retained_materialized_frames_when_a_pending_resume_disconnects() {
    // Arrange
    let limits = CassieRuntimeLimits::default();
    let producer = QueryExecutionControls::from_limits(&limits, Instant::now());
    let (mut state, portal) = state(&limits);
    runtime().block_on(suspend(
        &mut tokio::io::sink(),
        &mut state,
        &portal,
        result(&producer),
    ));
    let portal_controls = state.portal_memory_controls.clone();
    let (portal, suspended) = take_portal_execution(&mut state, "retained").expect("resume");
    let mut writer = PendingWrite;
    let mut write = Box::pin(write_suspended_result(
        &mut writer,
        SuspendedPortalWriteRequest {
            state: &mut state,
            portal_name: "retained",
            portal: &portal,
            suspended: suspended.expect("suspended result and frames"),
            active_cancellation: None,
            max_rows: 1,
            result_cap: 100,
        },
    ));
    let mut context = Context::from_waker(Waker::noop());

    // Act
    let pending = write.as_mut().poll(&mut context).is_pending();
    let retained = portal_controls.current_query_memory_bytes();
    drop(write);
    drop(state);

    // Assert
    assert!(pending);
    assert!(retained > 0);
    assert_eq!(portal_controls.current_query_memory_bytes(), 0);
    assert_eq!(producer.current_query_memory_bytes(), 0);
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

#[test]
fn should_retain_preflight_frame_admission_with_the_suspended_result_owner() {
    // Arrange
    let limits = CassieRuntimeLimits::default();
    let producer = QueryExecutionControls::from_limits(&limits, Instant::now());
    let result = result(&producer);
    let (mut state, portal) = state(&limits);
    let mut writer = tokio::io::sink();

    // Act
    runtime().block_on(suspend(&mut writer, &mut state, &portal, result));
    let result_bytes = state.portals["retained"]
        .suspended
        .as_ref()
        .expect("suspended result")
        .result
        .admission()
        .bytes();
    let with_frames = state.portal_memory_controls.current_query_memory_bytes();
    drop(state.remove_portal("retained"));

    // Assert
    assert!(
        with_frames > result_bytes,
        "the complete-result frame lease must remain with the portal"
    );
    assert_eq!(state.portal_memory_controls.current_query_memory_bytes(), 0);
    assert_eq!(producer.current_query_memory_bytes(), 0);
}

#[test]
fn should_reuse_the_actual_preflight_frame_buffer_after_portal_suspension() {
    // Arrange
    use crate::pgwire::connection::result_frames::{
        frame_allocation_count, observe_frame_allocations,
    };
    let limits = CassieRuntimeLimits::default();
    let producer = QueryExecutionControls::from_limits(&limits, Instant::now());
    let result = result(&producer);
    let (mut state, portal) = state(&limits);
    let mut writer = tokio::io::sink();
    let _observation = observe_frame_allocations();

    // Act
    let (first, resumed) = runtime().block_on(async {
        suspend(&mut writer, &mut state, &portal, result).await;
        let first = frame_allocation_count();
        let (portal, suspended) = take_portal_execution(&mut state, "retained").expect("resume");
        write_suspended_result(
            &mut writer,
            SuspendedPortalWriteRequest {
                state: &mut state,
                portal_name: "retained",
                portal: &portal,
                suspended: suspended.expect("retained owner"),
                active_cancellation: None,
                max_rows: 1,
                result_cap: 100,
            },
        )
        .await
        .expect("second page");
        (first, frame_allocation_count())
    });

    // Assert
    assert!(first > 0, "preflight owns actual frame backing");
    assert_eq!(
        resumed, first,
        "resume reuses the retained backing allocation"
    );
}

#[test]
fn should_resume_with_retained_encoding_scratch_when_a_competing_owner_fills_the_budget() {
    // Arrange
    let limits = CassieRuntimeLimits::default();
    let producer = QueryExecutionControls::from_limits(&limits, Instant::now());
    let result = result(&producer);
    let (mut state, portal) = state(&limits);
    let mut writer = tokio::io::sink();
    runtime().block_on(suspend(&mut writer, &mut state, &portal, result));
    let competing_bytes = limits.query_memory_budget_bytes
        - state.portal_memory_controls.current_query_memory_bytes();
    let competing = state
        .portal_memory_controls
        .reserve_query_memory(competing_bytes)
        .expect("competing owner occupies exactly the remaining budget");

    // Act
    let (portal, suspended) = take_portal_execution(&mut state, "retained").expect("resume");
    let resumed = runtime().block_on(write_suspended_result(
        &mut writer,
        SuspendedPortalWriteRequest {
            state: &mut state,
            portal_name: "retained",
            portal: &portal,
            suspended: suspended.expect("retained owner"),
            active_cancellation: None,
            max_rows: 1,
            result_cap: 100,
        },
    ));
    drop(state.remove_portal("retained"));
    let after_portal = state.portal_memory_controls.current_query_memory_bytes();
    drop(competing);

    // Assert
    assert!(
        resumed.is_ok(),
        "a preflighted resume needs no new encoding admission: {resumed:?}"
    );
    assert_eq!(
        after_portal, competing_bytes,
        "only the competing owner remains"
    );
    assert_eq!(state.portal_memory_controls.current_query_memory_bytes(), 0);
    assert_eq!(producer.current_query_memory_bytes(), 0);
}
