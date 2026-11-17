//! Prepared-statement ownership and parameter admission helpers.

use super::{
    remove_portals_for_prepared_id, Cassie, CassieSession, PreparedStatement, RuntimeState,
    SessionState, OID_UNKNOWN,
};

pub(super) fn unknown_high_bit_projection_oid(
    parsed: &crate::sql::ast::ParsedStatement,
    parameter_type_oids: &[i32],
) -> Option<i32> {
    let crate::sql::ast::QueryStatement::Select(select) = &parsed.statement else {
        return None;
    };
    select.projection.iter().find_map(|item| {
        let crate::sql::ast::SelectItem::Expr {
            expr: crate::sql::ast::Expr::Param(index),
            ..
        } = item
        else {
            return None;
        };
        let oid = parameter_type_oids.get(*index).copied()?;
        unknown_high_bit_parameter_oid(oid)
    })
}

/// Treat the signed wire slot as its original u32 bits; only unresolved high-bit OIDs are unknown.
/// The private vector inference marker shares the bits of wire OID `0x8000_0000`, but
/// wire slots never carry that marker, so a client-supplied value is an unknown type.
pub(super) fn unknown_high_bit_parameter_oid(oid: i32) -> Option<i32> {
    (oid < 0
        && (oid == crate::sql::binder::VECTOR_PARAMETER_TYPE_OID
            || crate::sql::binder::parameter_data_type_for_oid(oid).is_none()))
    .then_some(oid)
}

pub(super) fn with_function_input_scope<R>(
    cassie: &Cassie,
    session: &CassieSession,
    inspect: impl FnOnce(&crate::catalog::FunctionInputScope<'_>) -> R,
) -> R {
    session.with_search_path(|search_path| {
        let scope = crate::catalog::FunctionInputScope {
            database: session
                .current_database()
                .unwrap_or(&cassie.default_database),
            search_path,
            user: &session.user,
            enforce_database_scope: cassie.database_catalog_enforced(),
        };
        inspect(&scope)
    })
}

/// Registers `prepared` under its name, closing the portals of any statement
/// it replaces.
pub(super) async fn store_prepared_statement(
    cassie: std::sync::Arc<Cassie>,
    session: &CassieSession,
    runtime: &RuntimeState,
    state: &mut SessionState,
    prepared: PreparedStatement,
) -> Result<(), super::ExtendedQueryError> {
    let cleanup_cassie = std::sync::Arc::clone(&cassie);
    let cleanup_name = prepared.name.clone();
    let registration_session = session.clone();
    let name = prepared.name.clone();
    let query = prepared.query.clone();
    let id = prepared.id;
    let connection_name = prepared.name.clone();
    state.prepared_statements.try_reserve(1).map_err(|error| {
        super::ExtendedQueryError::cassie(&crate::app::CassieError::ResourceLimit(format!(
            "prepared statement directory allocation failed: {error}"
        )))
    })?;
    super::run_pgwire_blocking(cassie, "pgwire_register_prepared", move |worker_cassie| {
        #[cfg(not(test))]
        drop(worker_cassie);
        registration_session.register_wire_prepared(&name, id, &query)?;
        #[cfg(test)]
        registry_publication_tests::after_registry(&worker_cassie, &registration_session);
        Ok(())
    })
    .await
    .map_err(|error| super::ExtendedQueryError::cassie(&error))?;
    let publication = session.try_with_current_operation_owner(|_| {
        let replaced_prepared = state
            .prepared_statements
            .get(&prepared.name)
            .map(|statement| statement.id);
        if let Some(prepared_id) = replaced_prepared {
            remove_portals_for_prepared_id(state, runtime, prepared_id);
            state.prepared_result_descriptors.remove(&prepared_id);
        }
        state.prepared_statements.insert(connection_name, prepared);
        if replaced_prepared.is_none() {
            runtime.record_pgwire_prepared_delta(1);
        }
    });
    if let Err(error) = publication {
        if !cleanup_name.is_empty() {
            let cleanup_session = session.without_operation_failure_authority();
            super::run_pgwire_blocking(cleanup_cassie, "pgwire_forget_rejected_parse", move |_| {
                cleanup_session.forget_wire_prepared(&cleanup_name, id)
            })
            .await
            .map_err(|error| super::ExtendedQueryError::cassie(&error))?;
        }
        return Err(super::ExtendedQueryError::cassie(&error));
    }
    Ok(())
}

pub(super) fn normalize_parameter_type_oids(parameter_type_oids: &[i32]) -> Vec<i32> {
    parameter_type_oids
        .iter()
        .map(|oid| if *oid == 0 { OID_UNKNOWN } else { *oid })
        .collect()
}

#[cfg(test)]
#[path = "registry_publication_tests.rs"]
pub(super) mod registry_publication_tests;
