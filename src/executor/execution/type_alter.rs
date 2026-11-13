//! `ALTER TYPE` for composite types (#828).
//!
//! Every change publishes a new immutable descriptor revision, then re-embeds
//! it in the objects that carry a copy. Rows written under earlier revisions
//! stay readable because the physical slot list only grows (see
//! `catalog::composite_alter`). A structural change that would need stored
//! values rewritten is rejected before any mutation.

use super::command_output::empty as empty_command;
use super::type_command::{diagnostic, reject_composite_array, relation_name_occupied, ROOT_OID};
use super::{Cassie, QueryError, QueryResult};
use crate::catalog::composite_alter::{DescriptorPropagation, TypeAlterError};
use crate::catalog::{
    canonical_relation_name, canonical_schema_name, is_system_schema, CompositeTypeMeta,
    DEFAULT_SCHEMA,
};
use crate::runtime::QueryExecutionControls;
use crate::sql::ast::TypeAlteration;

/// The session context a type name resolves in.
pub(super) struct AlterScope<'a> {
    pub database: &'a str,
    pub search_order: &'a [&'a str],
}

fn alter_error(error: TypeAlterError) -> QueryError {
    diagnostic(error.sqlstate, error.message)
}

/// The type `raw` names, or the PostgreSQL error for a missing or non-composite target.
fn resolve_target(
    cassie: &Cassie,
    scope: &AlterScope<'_>,
    raw: &str,
) -> Result<CompositeTypeMeta, QueryError> {
    if let Some(metadata) =
        cassie
            .catalog
            .resolve_composite_type(scope.database, scope.search_order, raw)
    {
        return Ok(metadata);
    }
    let names_a_relation = scope.search_order.iter().any(|schema| {
        let canonical = canonical_relation_name(scope.database, schema, raw);
        cassie.catalog.get_schema(&canonical).is_some()
            || cassie.catalog.relation_exists(&canonical)
    });
    if names_a_relation {
        return Err(diagnostic(
            "42809",
            format!("\"{raw}\" is not a composite type"),
        ));
    }
    Err(diagnostic(
        "42704",
        format!("type \"{raw}\" does not exist"),
    ))
}

pub(super) fn alter(
    cassie: &Cassie,
    scope: &AlterScope<'_>,
    raw_name: &str,
    alteration: &TypeAlteration,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let current = resolve_target(cassie, scope, raw_name)?;
    let output = empty_command("ALTER TYPE", controls)?;
    match alteration {
        TypeAlteration::RenameTo { new_name } => {
            let canonical =
                canonical_relation_name(scope.database, &current.schema_name(), new_name);
            if relation_name_occupied(cassie, scope.database, &canonical, controls)? {
                return Err(diagnostic(
                    "42P07",
                    format!("relation \"{new_name}\" already exists"),
                ));
            }
            publish(cassie, &current, &current.with_name(canonical))?;
        }
        TypeAlteration::SetSchema { schema } => {
            set_schema(cassie, scope, &current, schema, controls)?;
        }
        TypeAlteration::OwnerTo { owner } => owner_to(cassie, &current, owner)?,
        TypeAlteration::RenameAttribute { from, to } => {
            let next = current
                .with_renamed_attribute(from, to)
                .map_err(alter_error)?;
            publish(cassie, &current, &next)?;
        }
        TypeAlteration::AddAttribute { attribute } => {
            reject_composite_array(&attribute.data_type)?;
            let next = current
                .with_added_attribute(&attribute.name, attribute.data_type.clone())
                .map_err(alter_error)?;
            publish(cassie, &current, &next)?;
        }
        TypeAlteration::DropAttribute { name, if_exists } => {
            match current.with_dropped_attribute(name) {
                Ok(next) => publish(cassie, &current, &next)?,
                Err(error) if *if_exists && error.sqlstate == "42703" => {}
                Err(error) => return Err(alter_error(error)),
            }
        }
        TypeAlteration::AlterAttributeType { name, data_type } => {
            reject_composite_array(data_type)?;
            let next = current
                .with_retyped_attribute(name, data_type.clone())
                .map_err(alter_error)?;
            reject_when_stored(cassie, &current)?;
            publish(cassie, &current, &next)?;
        }
    }
    Ok(output)
}

/// Retyping an attribute would have to rewrite stored values, so it is only
/// possible while no table column holds the type (PostgreSQL's rule too).
fn reject_when_stored(cassie: &Cassie, current: &CompositeTypeMeta) -> Result<(), QueryError> {
    let stored = cassie.catalog.composite_type_stored_in(current.type_oid());
    if let Some(column) = stored.first() {
        return Err(diagnostic(
            "0A000",
            format!(
                "cannot alter type \"{}\" because column \"{column}\" uses it",
                current.local_name()
            ),
        ));
    }
    Ok(())
}

fn set_schema(
    cassie: &Cassie,
    scope: &AlterScope<'_>,
    current: &CompositeTypeMeta,
    schema: &str,
    controls: &QueryExecutionControls,
) -> Result<(), QueryError> {
    if is_system_schema(schema) {
        return Err(diagnostic(
            "42501",
            format!("permission denied to move type into schema \"{schema}\""),
        ));
    }
    if !(schema.eq_ignore_ascii_case(DEFAULT_SCHEMA)
        || cassie
            .catalog
            .namespace_exists(&canonical_schema_name(scope.database, schema)))
    {
        return Err(diagnostic(
            "3F000",
            format!("schema \"{schema}\" does not exist"),
        ));
    }
    if schema.eq_ignore_ascii_case(&current.schema_name()) {
        return Ok(());
    }
    let canonical = canonical_relation_name(scope.database, schema, &current.local_name());
    if relation_name_occupied(cassie, scope.database, &canonical, controls)? {
        return Err(diagnostic(
            "42710",
            format!(
                "type \"{}\" already exists in schema \"{schema}\"",
                current.local_name()
            ),
        ));
    }
    publish(cassie, current, &current.with_name(canonical))
}

fn owner_to(cassie: &Cassie, current: &CompositeTypeMeta, owner: &str) -> Result<(), QueryError> {
    let role = cassie
        .catalog
        .get_role(owner)
        .ok_or_else(|| diagnostic("42704", format!("role \"{owner}\" does not exist")))?;
    let owner_oid = if role.is_admin && role.object_id == 0 {
        ROOT_OID
    } else {
        crate::catalog::object_identity::normal_oid(role.object_id).unwrap_or(ROOT_OID)
    };
    let mut next = current.clone();
    next.owner_oid = owner_oid;
    cassie
        .midge
        .put_composite_type(&next)
        .map_err(QueryError::from)?;
    cassie.catalog.register_composite_type(next);
    Ok(())
}

/// Persists `next`, re-embeds it everywhere it is carried and swaps the catalog.
///
/// Dependents are published before the type itself, so a failure part way
/// leaves the old type readable through objects that still carry its
/// descriptor. The steps are individually atomic, not one transaction.
fn publish(
    cassie: &Cassie,
    current: &CompositeTypeMeta,
    next: &CompositeTypeMeta,
) -> Result<(), QueryError> {
    apply_propagation(
        cassie,
        cassie
            .catalog
            .propagate_composite_change(&next.descriptor()),
    )?;
    cassie
        .midge
        .put_composite_type(next)
        .map_err(QueryError::from)?;
    if !current.name.eq_ignore_ascii_case(&next.name) {
        cassie
            .midge
            .delete_composite_type(&current.database, &current.name)
            .map_err(QueryError::from)?;
        cassie.catalog.unregister_composite_type(&current.name);
    }
    cassie.catalog.register_composite_type(next.clone());
    Ok(())
}

/// Publishes every embedded copy of a changed descriptor: table columns,
/// view columns and the fields of other composite types.
pub(super) fn apply_propagation(
    cassie: &Cassie,
    propagation: DescriptorPropagation,
) -> Result<(), QueryError> {
    for column in &propagation.columns {
        cassie
            .midge
            .alter_collection_column_type(&column.table, &column.column, column.data_type.clone())
            .map_err(QueryError::from)?;
        cassie.catalog.set_collection_field_type(
            &column.table,
            &column.column,
            column.data_type.clone(),
        );
    }
    for view in propagation.views {
        cassie.midge.put_view(&view).map_err(QueryError::from)?;
        cassie.catalog.register_view(view);
    }
    for dependent in propagation.types {
        cassie
            .midge
            .put_composite_type(&dependent)
            .map_err(QueryError::from)?;
        cassie.catalog.register_composite_type(dependent);
    }
    Ok(())
}

/// `DROP TABLE` is refused while another column or composite type holds the
/// table's row type (`2BP01`). `CASCADE` would drop those columns, which is
/// not implemented, so it is `0A000`.
pub(super) fn reject_row_type_dependents(
    cassie: &Cassie,
    table: &str,
    cascade: bool,
) -> Result<(), QueryError> {
    let Some(row_type) = cassie.catalog.relation_row_type(table) else {
        return Ok(());
    };
    let dependents = cassie
        .catalog
        .composite_type_dependents_with_owner(row_type.oid)
        .into_iter()
        .filter(|(owner, _)| !owner.eq_ignore_ascii_case(table))
        .map(|(_, phrase)| phrase)
        .collect::<Vec<_>>();
    if dependents.is_empty() {
        return Ok(());
    }
    if cascade {
        return Err(diagnostic(
            "0A000",
            format!(
                "DROP TABLE ... CASCADE of \"{}\" with columns of its row type is not supported",
                row_type.name
            ),
        ));
    }
    Err(QueryError::from(crate::app::CassieError::from(
        crate::app::DiagnosticError::new(
            "2BP01",
            format!(
                "cannot drop table {} because other objects depend on it",
                row_type.name
            ),
        )
        .with_detail(
            dependents
                .iter()
                .map(|phrase| format!("{phrase} depends on type {}", row_type.name))
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .with_hint("Use DROP ... CASCADE to drop the dependent objects too."),
    )))
}

/// Dropping or retyping a column of a table whose row type is stored elsewhere
/// would have to rewrite those stored values, so it is rejected before any
/// mutation (`0A000`, as PostgreSQL does for a retype).
pub(super) fn reject_drop_column_in_use(
    cassie: &Cassie,
    table: &str,
    operation: &crate::sql::ast::AlterTableOperation,
) -> Result<(), QueryError> {
    use crate::sql::ast::AlterTableOperation;
    let changes_layout = |inner: &AlterTableOperation| {
        matches!(
            inner,
            AlterTableOperation::DropColumn { .. } | AlterTableOperation::AlterColumnType { .. }
        )
    };
    let drops = match operation {
        AlterTableOperation::Multiple { operations } => operations.iter().any(changes_layout),
        other => changes_layout(other),
    };
    if !drops {
        return Ok(());
    }
    let Some(row_type) = cassie.catalog.relation_row_type(table) else {
        return Ok(());
    };
    if let Some(column) = cassie
        .catalog
        .composite_type_stored_in(row_type.oid)
        .first()
    {
        return Err(diagnostic(
            "0A000",
            format!(
                "cannot alter table \"{}\" because column \"{column}\" uses its row type",
                row_type.name
            ),
        ));
    }
    Ok(())
}

/// Re-embeds the row type of the table with `storage_id` after `ALTER TABLE`
/// added or renamed a column, or renamed or moved the table.
pub(super) fn refresh_relation_row_type(
    cassie: &Cassie,
    storage_id: u64,
) -> Result<(), QueryError> {
    let Some(collection) = cassie
        .catalog
        .list_collections()
        .into_iter()
        .find(|collection| collection.storage_id == storage_id)
    else {
        return Ok(());
    };
    // The persisted schema version is bumped by every column change.
    if let Some(projection) = cassie
        .midge
        .projection_metadata(&collection.name)
        .map_err(QueryError::from)?
    {
        cassie
            .catalog
            .set_row_type_revision(&collection.name, u64::from(projection.schema_version));
    }
    let Some(row_type) = cassie.catalog.relation_row_type(&collection.name) else {
        return Ok(());
    };
    apply_propagation(cassie, cassie.catalog.propagate_composite_change(&row_type))
}
