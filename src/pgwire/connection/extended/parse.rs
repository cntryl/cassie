//! Cancellable Parse admission before prepared-statement publication.

use super::{
    normalize_parameter_type_oids, parse_analysis, run_pgwire_blocking, store_prepared_statement,
    unsupported_sql_error, write_parse_complete, Arc, AsyncWrite, Cassie, CassieError,
    CassieSession, ExtendedQueryError, PreparedStatement, RuntimeState, SessionState,
};

const MAX_PREPARED_STATEMENTS_PER_CONNECTION: usize = 1_024;

pub(super) struct ParseRequest {
    pub(super) name: String,
    pub(super) query: String,
    pub(super) parameter_type_oids: Vec<i32>,
}

pub(super) async fn handle_parse(
    cassie: Arc<Cassie>,
    runtime: &RuntimeState,
    write_half: &mut (impl AsyncWrite + Unpin),
    state: &mut SessionState,
    session: &CassieSession,
    request: ParseRequest,
) -> Result<(), ExtendedQueryError> {
    let ParseRequest {
        name,
        query,
        parameter_type_oids,
    } = request;
    validate_prepared_name(state, session, &name)?;
    let empty_query = super::super::simple_query::split_simple_query(&query)
        .is_ok_and(|statements| statements.is_empty());
    if empty_query {
        let parameter_types = normalize_parameter_type_oids(&parameter_type_oids);
        let id = state.next_prepared_id();
        store_prepared_statement(
            runtime,
            state,
            PreparedStatement {
                id,
                name,
                query,
                parsed: None,
                sql_fingerprint: 0,
                parameter_count: parameter_types.len(),
                parameter_types,
                described: false,
            },
        );
        let _ = write_parse_complete(write_half).await;
        return Ok(());
    }
    if let Some(error) = unsupported_sql_error(&query) {
        if session.is_authenticated_read_only() {
            return Err(ExtendedQueryError::cassie(
                &CassieError::InsufficientPrivilege,
            ));
        }
        return Err(ExtendedQueryError::cassie(&error));
    }

    runtime.record_sql_parse();
    let parsed = crate::sql::parse_statement(&query).map_err(CassieError::from)?;
    session
        .authorize_statement(&parsed.statement)
        .map_err(|error| ExtendedQueryError::cassie(&error))?;
    let (parsed, parameter_types) = if session.repeatable_read_capture_generation().is_some() {
        let registration = state
            .backend_registration
            .as_ref()
            .ok_or_else(|| ExtendedQueryError::protocol("backend is not registered"))?;
        let cancellation = registration.begin_query();
        let handle = cancellation.handle();
        let session = session.clone();
        run_pgwire_blocking(cassie, "pgwire_parse_snapshot", move |cassie| {
            let started = std::time::Instant::now();
            let mut controls = crate::runtime::QueryExecutionControls::with_cancellation(
                &cassie.runtime.limits(),
                started,
                handle,
            );
            session.apply_statement_timeout(&mut controls, started);
            let result = (|| {
                cassie.prepare_transaction_analysis_view(&session, &parsed, false, &controls)?;
                admit(&cassie, &session, parsed, &parameter_type_oids, &controls)
            })();
            cassie.runtime.record_query_memory(&controls);
            Ok(result)
        })
        .await
        .map_err(|error| ExtendedQueryError::cassie(&error))??
    } else {
        let controls = runtime.query_controls(std::time::Instant::now());
        admit(&cassie, session, parsed, &parameter_type_oids, &controls)?
    };
    let sql_fingerprint = crate::runtime::sql_fingerprint(&parsed);
    let id = state.next_prepared_id();
    store_prepared_statement(
        runtime,
        state,
        PreparedStatement {
            id,
            name,
            query,
            parsed: Some(parsed),
            sql_fingerprint,
            parameter_count: parameter_types.len(),
            parameter_types,
            described: false,
        },
    );

    let _ = write_parse_complete(write_half).await;
    Ok(())
}

fn validate_prepared_name(
    state: &SessionState,
    session: &CassieSession,
    name: &str,
) -> Result<(), ExtendedQueryError> {
    if !name.is_empty() && state.prepared_statements.contains_key(name) {
        return Err(ExtendedQueryError::protocol(format!(
            "prepared statement '{name}' already exists"
        )));
    }
    if !name.is_empty() && session.has_sql_prepared(name) {
        return Err(ExtendedQueryError::cassie(&CassieError::from(
            crate::app::SessionObjectError::duplicate_prepared(name),
        )));
    }
    if !state.prepared_statements.contains_key(name)
        && state.prepared_statements.len() >= MAX_PREPARED_STATEMENTS_PER_CONNECTION
    {
        return Err(ExtendedQueryError::cassie(&CassieError::ResourceLimit(
            "prepared statement limit exceeded".to_string(),
        )));
    }
    Ok(())
}

fn admit(
    cassie: &Cassie,
    session: &CassieSession,
    mut parsed: crate::sql::ParsedStatement,
    parameter_type_oids: &[i32],
    controls: &crate::runtime::QueryExecutionControls,
) -> Result<(crate::sql::ParsedStatement, Vec<i32>), ExtendedQueryError> {
    let admitted =
        parse_analysis::admit(cassie, session, &mut parsed, parameter_type_oids, controls)?;
    let parameter_types = admitted
        .into_vec()
        .map_err(|error| ExtendedQueryError::parameter_admission(error, controls))?;
    Ok((parsed, parameter_types))
}
