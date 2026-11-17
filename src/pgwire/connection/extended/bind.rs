//! Bind preparation publishes only under the captured frame owner.

use super::{
    bind_parameters, bind_planning, check_prepared_storage_admission, current_catalog_version,
    describe_prepared, missing_statement_error, session_vector_oid, validate_format_codes,
    validate_format_count, validate_prepared_result_types, validate_result_formats,
    write_bind_complete, Arc, AsyncWrite, Cassie, CassieSession, ExtendedQueryError, Portal,
    RuntimeState, SessionState, MAX_PORTALS_PER_CONNECTION,
};

pub(super) struct BindRequest {
    pub(super) portal: String,
    pub(super) statement: String,
    pub(super) parameter_formats: Vec<i16>,
    pub(super) parameters: Vec<Option<Vec<u8>>>,
    pub(super) result_formats: Vec<i16>,
}

pub(super) async fn handle_bind(
    cassie: Arc<Cassie>,
    runtime: &RuntimeState,
    write_half: &mut (impl AsyncWrite + Unpin),
    state: &mut SessionState,
    session: &CassieSession,
    bind: BindRequest,
) -> Result<(), ExtendedQueryError> {
    let BindRequest {
        portal,
        statement,
        parameter_formats,
        parameters,
        result_formats,
    } = bind;
    validate_format_codes(&parameter_formats, "parameter")?;
    validate_format_codes(&result_formats, "result")?;
    let prepared = state
        .prepared_statements
        .get(&statement)
        .cloned()
        .ok_or_else(|| missing_statement_error(&statement))?;
    check_prepared_storage_admission(&cassie, &prepared)?;
    let previous_descriptor = state.prepared_result_descriptors.get(&prepared.id).cloned();
    let catalog_version = current_catalog_version(&cassie, session);
    let descriptor_is_stale = previous_descriptor.as_ref().is_some_and(|descriptor| {
        session.transaction_catalog_overlay().is_some()
            || descriptor.catalog_version() != catalog_version
    });
    let columns = if !result_formats.is_empty() || descriptor_is_stale {
        Some(describe_prepared(Arc::clone(&cassie), session.clone(), prepared.clone()).await?)
    } else {
        None
    };
    if let Some(columns) = columns.as_deref() {
        validate_prepared_result_types(previous_descriptor.as_deref(), columns)?;
        if let Some(descriptor) = previous_descriptor.as_ref() {
            descriptor.set_catalog_version(catalog_version);
        }
    }
    if !result_formats.is_empty() {
        let columns = columns
            .as_deref()
            .expect("nonempty result formats describe the prepared statement");
        validate_result_formats(columns, &result_formats)
            .map_err(|error| ExtendedQueryError::protocol_from_io(&error))?;
    }
    if parameters.len() != prepared.parameter_count {
        return Err(ExtendedQueryError::protocol(format!(
            "bind message supplies {} parameters, but prepared statement \"{}\" requires {}",
            parameters.len(),
            prepared.name,
            prepared.parameter_count
        )));
    }
    validate_format_count(parameter_formats.len(), parameters.len(), "parameter")?;

    let vector_oid = session_vector_oid(&cassie, session);
    let user_types = bind_parameters::user_parameters(&cassie, session, &prepared.parameter_types);
    let (values, parameter_memory) = bind_parameters::decode(
        (&parameters, &parameter_formats, &prepared.parameter_types),
        &state.portal_memory_controls,
        (vector_oid, &user_types),
    )?;

    let prepared_id = prepared.id;
    let described = prepared.described;
    let (values, read_view) =
        bind_planning::validate(cassie, session, state, prepared, values).await?;

    session
        .try_with_current_operation_owner(|identity| {
            state.reconcile_portals(identity, runtime);
            state.publish_bound_portal(
                Portal {
                    name: portal,
                    statement_name: statement,
                    prepared_id,
                    params: values,
                    parameter_memory,
                    result_formats,
                    described,
                    suspended: None,
                    completed_command: None,
                },
                identity,
                read_view,
                runtime,
                MAX_PORTALS_PER_CONNECTION,
            )
        })
        .map_err(|error| ExtendedQueryError::cassie(&error))?
        .map_err(|error| ExtendedQueryError::cassie(&error))?;

    let _ = write_bind_complete(write_half).await;
    Ok(())
}

#[cfg(test)]
#[path = "bind/ownership_tests.rs"]
mod ownership_tests;
