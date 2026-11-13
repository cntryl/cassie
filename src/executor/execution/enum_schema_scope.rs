//! Enum types and `DROP SCHEMA` (#827).

use super::schema_command::schema_namespace_command::object_in_schema;
use super::{Cassie, QueryError};
use crate::app::DiagnosticError;

/// Whether `schema` still declares an enum type.
pub(super) fn schema_has_enums(cassie: &Cassie, schema: &str) -> bool {
    cassie
        .catalog
        .list_enum_types()
        .iter()
        .any(|metadata| object_in_schema(&metadata.name, schema))
}

/// A cascade may drop a schema's enums only when no table outside the schema
/// still uses them; PostgreSQL reports such a dependent with `2BP01`.
pub(super) fn reject_outside_dependents(cassie: &Cassie, schema: &str) -> Result<(), QueryError> {
    for metadata in cassie
        .catalog
        .list_enum_types()
        .into_iter()
        .filter(|metadata| object_in_schema(&metadata.name, schema))
    {
        let name = metadata.local_name();
        let outside = cassie
            .catalog
            .enum_type_dependents(metadata.type_oid())
            .into_iter()
            .filter(|dependent| !object_in_schema(&dependent.owner, schema))
            .map(|dependent| dependent.detail(&name))
            .collect::<Vec<_>>();
        if !outside.is_empty() {
            return Err(QueryError::Cassie(
                DiagnosticError::new(
                    "2BP01",
                    format!("cannot drop type {name} because other objects depend on it"),
                )
                .with_detail(outside.join("\n"))
                .into(),
            ));
        }
    }
    Ok(())
}

/// Drops every enum type declared in `schema`.
pub(super) fn drop_schema_enums(cassie: &Cassie, schema: &str) -> Result<(), QueryError> {
    for metadata in cassie
        .catalog
        .list_enum_types()
        .into_iter()
        .filter(|metadata| object_in_schema(&metadata.name, schema))
    {
        cassie
            .midge
            .delete_enum_type(&metadata.database, &metadata.name)
            .map_err(QueryError::Cassie)?;
        cassie.catalog.unregister_enum_type(&metadata.name);
    }
    Ok(())
}
