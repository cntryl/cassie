//! Cancellable planning before a portal or `BindComplete` can become visible.
use super::{
    run_pgwire_blocking, Arc, Cassie, CassieSession, ExtendedQueryError, PreparedStatement,
    SessionState,
};

pub(super) async fn validate(
    cassie: Arc<Cassie>,
    session: &CassieSession,
    state: &SessionState,
    prepared: PreparedStatement,
) -> Result<(), ExtendedQueryError> {
    let Some(parsed) = prepared.parsed else {
        return Ok(());
    };
    let registration = state
        .backend_registration
        .as_ref()
        .ok_or_else(|| ExtendedQueryError::protocol("backend is not registered"))?;
    let cancellation = registration.begin_query();
    let handle = cancellation.handle();
    let session = session.clone();
    run_pgwire_blocking(cassie, "pgwire_bind_plan", move |cassie| {
        cassie.validate_pgwire_bind(
            &session,
            parsed,
            prepared.sql_fingerprint,
            &prepared.parameter_types,
            &handle,
        )
    })
    .await
    .map_err(|error| ExtendedQueryError::cassie(&error))
}
