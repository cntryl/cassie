//! Domain types and `DROP SCHEMA` (#828).

use super::schema_command::schema_namespace_command::object_in_schema;
use super::{Cassie, QueryError};
use crate::app::DiagnosticError;

/// Whether `schema` still declares a domain.
pub(super) fn schema_has_domains(cassie: &Cassie, schema: &str) -> bool {
    cassie
        .catalog
        .list_domain_types()
        .iter()
        .any(|domain| object_in_schema(&domain.name, schema))
}

/// A cascade may drop a schema's domains only when no table outside the schema
/// still uses them; PostgreSQL reports such a dependent with `2BP01`.
pub(super) fn reject_outside_dependents(cassie: &Cassie, schema: &str) -> Result<(), QueryError> {
    for domain in cassie
        .catalog
        .list_domain_types()
        .into_iter()
        .filter(|domain| object_in_schema(&domain.name, schema))
    {
        let outside = cassie
            .catalog
            .domain_type_dependents_with_owner(domain.type_oid())
            .into_iter()
            .filter(|(owner, _)| !object_in_schema(owner, schema))
            .map(|(_, phrase)| format!("{phrase} depends on type {}", domain.local_name()))
            .collect::<Vec<_>>();
        if !outside.is_empty() {
            return Err(QueryError::Cassie(
                DiagnosticError::new(
                    "2BP01",
                    format!(
                        "cannot drop type {} because other objects depend on it",
                        domain.local_name()
                    ),
                )
                .with_detail(outside.join("\n"))
                .into(),
            ));
        }
    }
    Ok(())
}

/// Drops every domain declared in `schema`.
pub(super) fn drop_schema_domains(cassie: &Cassie, schema: &str) -> Result<(), QueryError> {
    for domain in cassie
        .catalog
        .list_domain_types()
        .into_iter()
        .filter(|domain| object_in_schema(&domain.name, schema))
    {
        // The cascade reaches views that cast to the domain, in any schema.
        super::domain_view_dependents::drop_dependent_views(cassie, &domain)?;
        cassie
            .midge
            .delete_domain_type(&domain.database, &domain.name)
            .map_err(QueryError::Cassie)?;
        cassie.catalog.unregister_domain_type(&domain.name);
    }
    Ok(())
}
