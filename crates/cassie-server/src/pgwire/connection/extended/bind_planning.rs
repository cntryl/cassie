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
    parameters: Vec<crate::types::Value>,
) -> Result<
    (
        Vec<crate::types::Value>,
        Option<Arc<crate::app::BoundReadView>>,
    ),
    ExtendedQueryError,
> {
    let Some(parsed) = prepared.parsed else {
        return Ok((parameters, None));
    };
    let registration = state
        .backend_registration
        .as_ref()
        .ok_or_else(|| ExtendedQueryError::protocol("backend is not registered"))?;
    let cancellation = registration.begin_query();
    let handle = cancellation.handle();
    let session = session.clone();
    let retention = state.portal_memory_controls.clone();
    run_pgwire_blocking(cassie, "pgwire_bind_plan", move |cassie| {
        let read = cassie.validate_pgwire_bind(
            &session,
            parsed,
            prepared.sql_fingerprint,
            &prepared.parameter_types,
            &parameters,
            (&handle, &retention),
        )?;
        Ok((parameters, read))
    })
    .await
    .map_err(|error| ExtendedQueryError::cassie(&error))
}
