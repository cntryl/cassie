//! One extended-protocol message retains its operation through async output.
use super::{
    dispatch_message, read_frontend_message, write_error_response, Arc, AsyncWrite, Cassie,
    CassieSession, ConnectionStep, FrontendMessage, HandshakeError, HandshakeState, PgWireError,
    PgwireReader, RuntimeState, SessionState,
};

pub(super) enum DispatchOutcome {
    Continue,
    Break,
}

pub(in crate::pgwire::connection) async fn handle_frontend_message(
    cassie: Arc<Cassie>,
    runtime: &RuntimeState,
    reader: &mut PgwireReader,
    write_half: &mut (impl AsyncWrite + Unpin),
    state: &mut SessionState,
    awaiting_sync: &mut bool,
    session: &CassieSession,
) -> ConnectionStep {
    let message = match read_frontend_message(reader).await {
        Ok(message) => message,
        Err(HandshakeError::Closed) => return ConnectionStep::Break,
        Err(HandshakeError::Invalid(error)) => {
            runtime.record_pgwire_protocol_error();
            let operation = session.with_new_wire_operation();
            super::super::transactions::capture_protocol_owner(&operation).await;
            super::super::transactions::fail_owned_query(runtime, state, &operation).await;
            let _ = write_error_response(
                write_half,
                &PgWireError::protocol(format!("invalid frontend message: {error}")),
            )
            .await;
            *awaiting_sync = true;
            return ConnectionStep::Continue(HandshakeState::Ready);
        }
    };

    let operation_session = session.with_new_wire_operation();
    let session = &operation_session;
    if !matches!(&message, FrontendMessage::Execute { .. }) {
        super::super::transactions::capture_protocol_owner(session).await;
    }
    let invalidate_portals_on_error = matches!(
        &message,
        FrontendMessage::Parse { query, .. }
            if query
                .trim_start()
                .get(..5)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case("copy "))
    );
    let dispatched = dispatch_message(
        Arc::clone(&cassie),
        runtime,
        write_half,
        state,
        session,
        message,
    )
    .await;
    #[cfg(test)]
    super::super::wire_failure_test_hooks::after_worker(&cassie, session);
    match dispatched {
        Ok(DispatchOutcome::Continue) => ConnectionStep::Continue(HandshakeState::Ready),
        Ok(DispatchOutcome::Break) => ConnectionStep::Break,
        Err(error) => {
            if error.record_protocol_error {
                runtime.record_pgwire_protocol_error();
            }
            super::super::transactions::capture_protocol_owner(session).await;
            super::super::transactions::fail_owned_query(runtime, state, session).await;
            if super::super::writers::write_changed_parameter_statuses(
                write_half,
                session,
                &mut state.parameter_statuses,
            )
            .await
            .is_err()
            {
                return ConnectionStep::Break;
            }
            let _ = write_error_response(write_half, error.pg_error.as_ref()).await;
            state.invalidate_portals_on_sync |= invalidate_portals_on_error;
            if invalidate_portals_on_error {
                state.portal_invalidation_owner = session.operation_failure_authority().cloned();
            }
            *awaiting_sync = true;
            ConnectionStep::Continue(HandshakeState::Ready)
        }
    }
}
