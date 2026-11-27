//! Table privilege checks shared by statement authorization and by functions
//! that read a table on the caller's behalf.
use super::CassieSession;
use crate::app::CassieError;
use crate::catalog::{RoleGrantTarget, RoleObjectPrivilege};

/// Whether `session` holds `privilege` on `relation`: schema `USAGE` plus the
/// table grant. Shared by statement authorization and by functions that read a
/// table on the caller's behalf.
pub(crate) fn table_privilege(
    catalog: &crate::catalog::Catalog,
    default_database: &str,
    session: &CassieSession,
    context: &crate::sql::binder::BindingContext,
    relation: &str,
    privilege: RoleObjectPrivilege,
) -> Result<(), CassieError> {
    let resolved = crate::sql::binder::resolve_relation_name(relation, catalog, context)?;
    resolved_table_privilege(catalog, default_database, session, &resolved, privilege)
}

/// [`table_privilege`] for a relation already resolved to its canonical name.
fn resolved_table_privilege(
    catalog: &crate::catalog::Catalog,
    default_database: &str,
    session: &CassieSession,
    resolved: &str,
    privilege: RoleObjectPrivilege,
) -> Result<(), CassieError> {
    let resolved = resolved.to_owned();
    let schema_name = crate::catalog::relation_schema_name(&resolved);
    // System relations are stored as `schema.name`, not database-qualified.
    let system_qualified = matches!(
        crate::catalog::parse_name(&resolved),
        Ok(crate::catalog::ParsedName::SchemaQualified { schema, .. })
            if crate::catalog::is_system_schema(&schema)
    );
    if system_qualified || crate::catalog::is_system_schema(&schema_name) {
        return Ok(());
    }

    let Some(collection) = catalog.get_collection(&resolved) else {
        return Err(CassieError::InsufficientPrivilege);
    };
    let database = crate::catalog::relation_database_name(&resolved)
        .or_else(|| session.current_database().map(str::to_owned))
        .unwrap_or_else(|| default_database.to_owned());
    if session.external_database_access(&database) == Some(false) {
        return Err(CassieError::InsufficientPrivilege);
    }
    let Some(database_meta) = catalog.get_database(&database) else {
        return Err(CassieError::InsufficientPrivilege);
    };
    let Some(schema) = catalog.get_namespace(&crate::catalog::canonical_schema_name(
        &database,
        &schema_name,
    )) else {
        return Err(CassieError::InsufficientPrivilege);
    };
    let Some(role) = session.catalog_role(catalog) else {
        return Err(CassieError::InsufficientPrivilege);
    };
    let schema_usage = catalog.get_role_object_grant(
        &database_meta.physical_family,
        role.object_id,
        &RoleGrantTarget::Schema(schema.object_id),
    );
    if !schema_usage.is_some_and(|grant| grant.grants(RoleObjectPrivilege::Usage)) {
        return Err(CassieError::InsufficientPrivilege);
    }
    let table_grant = catalog.get_role_object_grant(
        &database_meta.physical_family,
        role.object_id,
        &RoleGrantTarget::Table(collection.storage_id),
    );
    if table_grant.is_some_and(|grant| grant.grants(privilege)) {
        Ok(())
    } else {
        Err(CassieError::InsufficientPrivilege)
    }
}

#[doc(hidden)]
#[must_use]
pub fn relation_readable(
    catalog: &crate::catalog::Catalog,
    default_database: &str,
    session: &CassieSession,
    relation: &str,
) -> bool {
    if !session_is_unprivileged(catalog, session) {
        return true;
    }
    relation_readable_with_seen(
        catalog,
        default_database,
        session,
        relation,
        &mut std::collections::HashSet::new(),
    )
}

fn relation_readable_with_seen(
    catalog: &crate::catalog::Catalog,
    default_database: &str,
    session: &CassieSession,
    relation: &str,
    seen: &mut std::collections::HashSet<String>,
) -> bool {
    if !seen.insert(relation.to_ascii_lowercase()) {
        return true;
    }
    if catalog.get_collection(relation).is_some() {
        return resolved_table_privilege(
            catalog,
            default_database,
            session,
            relation,
            RoleObjectPrivilege::Select,
        )
        .is_ok();
    }
    let Some(view) = catalog.get_view(relation) else {
        return false;
    };
    let Some((_, context)) = session.object_lookup_context() else {
        return false;
    };
    let Ok(parsed) = crate::sql::parse_statement_in_catalog(catalog, &context, &view.query) else {
        return false;
    };
    let crate::sql::ast::QueryStatement::Select(select) = parsed.statement else {
        return false;
    };
    let mut graph_tables = Vec::new();
    for name in crate::sql::binder::select_referenced_graphs(&select) {
        let Ok(path) = crate::sql::ast::IdentifierPath::parse(&name) else {
            return false;
        };
        let Ok(graph) = crate::sql::binder::resolve_graph(catalog, &context, &path) else {
            return false;
        };
        graph_tables.extend(graph.mapped_tables().into_iter().map(str::to_owned));
    }
    let direct_readable = crate::sql::binder::select_referenced_relations(&select)
        .iter()
        .all(|name| {
            crate::sql::binder::normalize_relation_name(name, &context).is_ok_and(|name| {
                relation_readable_with_seen(catalog, default_database, session, &name, seen)
            })
        });
    direct_readable
        && graph_tables.iter().all(|table| {
            relation_readable_with_seen(catalog, default_database, session, table, seen)
        })
}

/// Read authority of `session` on `table` for a function that looks a row up
/// on the caller's behalf: embedded and administrative sessions may read, any
/// other role needs schema `USAGE` and table `SELECT`.
///
/// # Errors
/// Returns `InsufficientPrivilege` (`42501`) when the session may not read.
pub(crate) fn require_table_read(
    catalog: &crate::catalog::Catalog,
    session: &CassieSession,
    context: &crate::sql::binder::BindingContext,
    table: &str,
) -> Result<(), CassieError> {
    if !session_is_unprivileged(catalog, session) {
        return Ok(());
    }
    resolved_table_privilege(
        catalog,
        &context.database,
        session,
        table,
        RoleObjectPrivilege::Select,
    )
}

/// Whether `session` is a restricted role: a network-authenticated session
/// whose role is not an administrator. Embedded sessions are never restricted.
pub(crate) fn session_is_unprivileged(
    catalog: &crate::catalog::Catalog,
    session: &CassieSession,
) -> bool {
    session.is_network_authenticated()
        && !session.catalog_role(catalog).map_or_else(
            || !session.uses_aws_iam() && session.user.eq_ignore_ascii_case("root"),
            |role| role.is_admin,
        )
}
