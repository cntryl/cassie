//! Execute admission before cached, suspended, or fresh portal output.

use super::{
    current_catalog_version, describe_prepared, execute_prepared_portal,
    execute_streaming_portal_page, missing_portal_error, prepared_for_portal,
    resume_suspended_portal, streamable_portal_query, take_portal_execution, transaction_boundary,
    validate_prepared_result_types, write_completed_portal, write_empty_query_response, Arc,
    AsyncWrite, Cassie, CassieSession, ExtendedQueryError, PreparedPortalRequest, RuntimeState,
    SessionState, StreamingPortalPageRequest, SuspendedPortalRequest,
};

pub(super) async fn handle_execute(
    cassie: Arc<Cassie>,
    runtime: &RuntimeState,
    write_half: &mut (impl AsyncWrite + Unpin),
    state: &mut SessionState,
    session: &CassieSession,
    portal_name: String,
    max_rows: i32,
) -> Result<(), ExtendedQueryError> {
    runtime.record_pgwire_extended_query();
    let quarantined = admit_recovery_execute(&cassie, state, &portal_name)?;
    let (max_rows, first_execute) =
        transaction_boundary::prepare_execute(state, session, &portal_name, max_rows)?;
    let (portal, suspended) = take_portal_execution(state, &portal_name)
        .ok_or_else(|| missing_portal_error(&portal_name))?;

    if let Some(suspended) = suspended.filter(|_| !quarantined) {
        return resume_suspended_portal(
            cassie,
            write_half,
            SuspendedPortalRequest {
                state,
                session,
                portal_name: &portal_name,
                portal: &portal,
                suspended,
                max_rows,
            },
        )
        .await;
    }
    if let Some(command) = portal.completed_command.as_deref().filter(|_| !quarantined) {
        return write_completed_portal(write_half, command, &state.portal_memory_controls).await;
    }

    let prepared = prepared_for_portal(state, &portal)?;
    if let Some(descriptor) = state.prepared_result_descriptors.get(&prepared.id).cloned() {
        let catalog_version = current_catalog_version(&cassie, session);
        if session.transaction_catalog_overlay().is_some()
            || descriptor.catalog_version() != catalog_version
        {
            let columns =
                describe_prepared(Arc::clone(&cassie), session.clone(), prepared.clone()).await?;
            validate_prepared_result_types(Some(&descriptor), &columns)?;
            descriptor.set_catalog_version(catalog_version);
        }
    }
    let Some(parsed) = prepared.parsed.clone() else {
        return write_empty_query_response(write_half)
            .await
            .map_err(|error| ExtendedQueryError::write_failed(&error));
    };
    let standalone = transaction_boundary::admit_statement(
        state,
        session,
        &parsed,
        &cassie.catalog,
        first_execute,
    )?;
    if streamable_portal_query(&prepared, max_rows)
        && state.portal_reads.get(&portal_name).is_none()
    {
        return execute_streaming_portal_page(
            cassie,
            write_half,
            StreamingPortalPageRequest {
                state,
                session,
                portal_name: &portal_name,
                portal: &portal,
                prepared: &prepared,
                max_rows,
                rows_emitted: 0,
                cancellation: None,
            },
        )
        .await;
    }
    let result = execute_prepared_portal(
        cassie,
        runtime,
        write_half,
        state,
        session,
        PreparedPortalRequest {
            portal_name: &portal_name,
            portal: &portal,
            prepared: &prepared,
            parsed,
            max_rows,
            standalone,
        },
    )
    .await;
    if quarantined {
        state.clear_all_portals(runtime);
    }
    result
}

fn admit_recovery_execute(
    cassie: &Cassie,
    state: &SessionState,
    portal_name: &str,
) -> Result<bool, ExtendedQueryError> {
    let quarantined = cassie.check_storage_recovery_required().is_err();
    if quarantined {
        let parsed = state
            .portals
            .get(portal_name)
            .and_then(|portal| state.prepared_statements.get(&portal.statement_name))
            .and_then(|prepared| prepared.parsed.as_ref())
            .ok_or_else(|| {
                ExtendedQueryError::cassie(
                    &cassie
                        .check_storage_recovery_required()
                        .expect_err("latched recovery requirement"),
                )
            })?;
        cassie
            .check_storage_statement_admission(&parsed.statement)
            .map_err(|error| ExtendedQueryError::cassie(&error))?;
    }
    Ok(quarantined)
}
