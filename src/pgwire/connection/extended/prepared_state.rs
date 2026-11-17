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
    let registration_session = session.clone();
    let name = prepared.name.clone();
    let query = prepared.query.clone();
    let id = prepared.id;
    super::run_pgwire_blocking(cassie, "pgwire_register_prepared", move |_| {
        registration_session.register_wire_prepared(&name, id, &query)
    })
    .await
    .map_err(|error| super::ExtendedQueryError::cassie(&error))?;
    let replaced_prepared = state
        .prepared_statements
        .get(&prepared.name)
        .map(|statement| statement.id);
    if let Some(prepared_id) = replaced_prepared {
        remove_portals_for_prepared_id(state, runtime, prepared_id);
        state.prepared_result_descriptors.remove(&prepared_id);
    }
    state
        .prepared_statements
        .insert(prepared.name.clone(), prepared);
    if replaced_prepared.is_none() {
        runtime.record_pgwire_prepared_delta(1);
    }
    Ok(())
}

pub(super) fn normalize_parameter_type_oids(parameter_type_oids: &[i32]) -> Vec<i32> {
    parameter_type_oids
        .iter()
        .map(|oid| if *oid == 0 { OID_UNKNOWN } else { *oid })
        .collect()
}
