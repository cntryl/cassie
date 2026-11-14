//! `CREATE TYPE ... AS ENUM`, `ALTER TYPE` on enums and `DROP TYPE` of enums (#827).
//!
//! The design authority is the "Selected v0.4.0 named enum contract" section of
//! `docs/type-contract.md`. An alteration builds a new immutable descriptor
//! revision and publishes it together with every column and view that embeds
//! it, in one Midge transaction. Member ids, type ids and array ids come from
//! the persistent object allocator, which commits independently of the DDL
//! overlay: an id allocated by a statement that rolls back is burned.

use super::command_output::empty as empty_command;
use super::domain_command::type_name_taken;
use super::type_alter::{owner_oid_for, AlterScope};
use super::type_command::{creation_target, diagnostic};
use super::{Cassie, CassieSession, QueryError, QueryResult};
use crate::app::{CassieError, DiagnosticError};
use crate::catalog::composite_alter::TypeAlterError;
use crate::catalog::{
    canonical_relation_name, canonical_schema_name, is_system_schema, EnumTypeMeta, DEFAULT_SCHEMA,
    ENUM_TYPE_FORMAT_VERSION,
};
use crate::runtime::QueryExecutionControls;
use crate::sql::ast::TypeAlteration;
use crate::types::enum_type::{sort_order_at, EnumMember, MAX_ENUM_LABEL_BYTES};
use crate::types::EnumValuePlacement;

/// First revision of a newly created enum descriptor.
const INITIAL_REVISION: u64 = 1;

fn alter_error(error: TypeAlterError) -> QueryError {
    diagnostic(error.sqlstate, error.message)
}

/// `42602` for a label longer than 63 bytes, as PostgreSQL reports it.
fn validate_label(label: &str) -> Result<(), QueryError> {
    if label.len() <= MAX_ENUM_LABEL_BYTES {
        return Ok(());
    }
    Err(CassieError::from(
        DiagnosticError::new("42602", format!("invalid enum label \"{label}\"")).with_detail(
            format!("Labels must be {MAX_ENUM_LABEL_BYTES} bytes or less."),
        ),
    )
    .into())
}

/// Validates every label of `CREATE TYPE ... AS ENUM`: length first, then
/// uniqueness (`23505`, the violation of `pg_enum_typid_label_index`).
fn validate_labels(labels: &[String]) -> Result<(), QueryError> {
    for label in labels {
        validate_label(label)?;
    }
    for (index, label) in labels.iter().enumerate() {
        if labels[..index].contains(label) {
            return Err(diagnostic(
                "23505",
                "duplicate key value violates unique constraint \"pg_enum_typid_label_index\""
                    .into(),
            ));
        }
    }
    Ok(())
}

pub(super) fn create(
    cassie: &Cassie,
    database: &str,
    current_schema: &str,
    raw_name: &str,
    labels: &[String],
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let (schema, name) = creation_target(database, current_schema, raw_name)?;
    if is_system_schema(&schema) {
        return Err(diagnostic(
            "42501",
            format!("permission denied to create type in schema \"{schema}\""),
        ));
    }
    let canonical_schema = canonical_schema_name(database, &schema);
    if !(schema.eq_ignore_ascii_case(DEFAULT_SCHEMA)
        || cassie.catalog.namespace_exists(&canonical_schema))
    {
        return Err(diagnostic(
            "3F000",
            format!("schema \"{schema}\" does not exist"),
        ));
    }
    let canonical = canonical_relation_name(database, &schema, &name);
    if type_name_taken(cassie, &canonical) {
        return Err(diagnostic(
            "42710",
            format!("type \"{name}\" already exists"),
        ));
    }
    validate_labels(labels)?;
    let output = empty_command("CREATE TYPE", controls)?;
    let allocate = || cassie.midge.allocate_object_id().map_err(QueryError::from);
    let object_id = allocate()?;
    let array_object_id = allocate()?;
    let mut members = Vec::with_capacity(labels.len());
    for (position, label) in labels.iter().enumerate() {
        members.push(EnumMember {
            member_id: allocate()?,
            label: label.clone(),
            sort_order: sort_order_at(position),
            uncommitted: false,
        });
    }
    let metadata = EnumTypeMeta {
        format_version: ENUM_TYPE_FORMAT_VERSION,
        database: database.to_owned(),
        name: canonical,
        object_id,
        array_object_id,
        owner_oid: super::type_command::ROOT_OID,
        revision: INITIAL_REVISION,
        members,
    };
    cassie
        .midge
        .put_enum_type(&metadata)
        .map_err(QueryError::from)?;
    cassie.catalog.register_enum_type(metadata);
    Ok(output)
}

/// The enum type `raw` names when no composite or domain type of the same
/// name sits earlier in `search_order`. The three kinds share one namespace
/// per schema, so the first schema that declares the name decides its kind.
pub(super) fn resolve_first_in_path(
    cassie: &Cassie,
    database: &str,
    search_order: &[&str],
    raw: &str,
) -> Option<EnumTypeMeta> {
    let catalog = &cassie.catalog;
    if search_order.is_empty() {
        return catalog.resolve_enum_type(database, search_order, raw);
    }
    for schema in search_order {
        let scope = std::slice::from_ref(schema);
        if let Some(found) = catalog.resolve_enum_type(database, scope, raw) {
            return Some(found);
        }
        if catalog
            .resolve_composite_type(database, scope, raw)
            .is_some()
            || catalog.resolve_domain_type(database, scope, raw).is_some()
        {
            return None;
        }
    }
    None
}

/// `ALTER TYPE` when `raw` names an enum under the search-path rule of
/// [`resolve_first_in_path`]. Returns `None` otherwise.
pub(super) fn alter_if_enum(
    cassie: &Cassie,
    scope: &AlterScope<'_>,
    raw: &str,
    alteration: &TypeAlteration,
    controls: &QueryExecutionControls,
) -> Result<Option<QueryResult>, QueryError> {
    let Some(current) = resolve_first_in_path(cassie, scope.database, scope.search_order, raw)
    else {
        return Ok(None);
    };
    alter(cassie, scope, &current, alteration, controls).map(Some)
}

fn alter(
    cassie: &Cassie,
    scope: &AlterScope<'_>,
    current: &EnumTypeMeta,
    alteration: &TypeAlteration,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let output = empty_command("ALTER TYPE", controls)?;
    match alteration {
        TypeAlteration::RenameTo { new_name } => {
            let canonical =
                canonical_relation_name(scope.database, &current.schema_name(), new_name);
            if type_name_taken(cassie, &canonical) {
                return Err(diagnostic(
                    "42710",
                    format!("type \"{new_name}\" already exists"),
                ));
            }
            let views =
                super::enum_views::rename_type(cassie, scope.session, current, new_name, None);
            publish_with_views(cassie, current, &current.with_name(canonical), views)?;
        }
        TypeAlteration::SetSchema { schema } => set_schema(cassie, scope, current, schema)?,
        TypeAlteration::OwnerTo { owner } => {
            let mut next = current.clone();
            next.owner_oid = owner_oid_for(cassie, owner)?;
            cassie
                .midge
                .put_enum_type(&next)
                .map_err(QueryError::from)?;
            cassie.catalog.register_enum_type(next);
        }
        TypeAlteration::AddValue {
            label,
            if_not_exists,
            placement,
        } => add_value(
            cassie,
            scope.session,
            current,
            label,
            *if_not_exists,
            placement.as_ref(),
        )?,
        TypeAlteration::RenameValue { from, to } => {
            validate_label(to)?;
            let next = current.with_renamed_value(from, to).map_err(alter_error)?;
            let views = super::enum_views::rename_label(cassie, scope.session, current, from, to);
            publish_with_views(cassie, current, &next, views)?;
        }
        TypeAlteration::RenameAttribute { .. }
        | TypeAlteration::AddAttribute { .. }
        | TypeAlteration::DropAttribute { .. }
        | TypeAlteration::AlterAttributeType { .. } => {
            // PostgreSQL resolves attribute actions as relation lookups.
            return Err(diagnostic(
                "42P01",
                format!("relation \"{}\" does not exist", current.local_name()),
            ));
        }
    }
    Ok(output)
}

/// Whether `session` is inside an explicit transaction that began before the
/// type `current` was published (`ADD VALUE` labels are then uncommitted).
fn existed_before_transaction(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    current: &EnumTypeMeta,
) -> bool {
    session.is_some_and(CassieSession::is_transaction_active)
        && cassie
            .publication_catalog
            .get_enum_type_by_oid(current.type_oid())
            .is_some()
}

fn add_value(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    current: &EnumTypeMeta,
    label: &str,
    if_not_exists: bool,
    placement: Option<&EnumValuePlacement>,
) -> Result<(), QueryError> {
    validate_label(label)?;
    if if_not_exists && current.members.iter().any(|member| member.label == label) {
        // The NOTICE `enum label "x" already exists, skipping` is emitted by
        // the wire layer from the same catalog state.
        return Ok(());
    }
    let plan = current
        .plan_added_value(label, placement)
        .map_err(alter_error)?;
    let member_id = cassie
        .midge
        .allocate_object_id()
        .map_err(QueryError::from)?;
    let uncommitted = existed_before_transaction(cassie, session, current);
    let next = current.with_added_value(plan, label.to_owned(), member_id, uncommitted);
    publish(cassie, current, &next)
}

fn set_schema(
    cassie: &Cassie,
    scope: &AlterScope<'_>,
    current: &EnumTypeMeta,
    schema: &str,
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
    if type_name_taken(cassie, &canonical) {
        return Err(diagnostic(
            "42710",
            format!(
                "type \"{}\" already exists in schema \"{schema}\"",
                current.local_name()
            ),
        ));
    }
    let views = super::enum_views::rename_type(
        cassie,
        scope.session,
        current,
        &current.local_name(),
        Some(schema),
    );
    publish_with_views(cassie, current, &current.with_name(canonical), views)
}

/// Persists `next`, re-embeds it in every column and view that carries a copy,
/// and swaps the catalog. The type and its dependents commit in one Midge
/// transaction.
fn publish(cassie: &Cassie, current: &EnumTypeMeta, next: &EnumTypeMeta) -> Result<(), QueryError> {
    publish_with_views(cassie, current, next, Vec::new())
}

/// [`publish`], also storing the view definitions a rename rewrote. A view is
/// re-bound from its text, so the text must follow the new label or name.
fn publish_with_views(
    cassie: &Cassie,
    current: &EnumTypeMeta,
    next: &EnumTypeMeta,
    rewritten: Vec<super::enum_views::RewrittenView>,
) -> Result<(), QueryError> {
    let renamed = !current.name.eq_ignore_ascii_case(&next.name);
    let mut propagation = cassie.catalog.propagate_enum_change(&next.descriptor());
    for view in rewritten {
        if let Some(embedded) = propagation
            .views
            .iter_mut()
            .find(|embedded| embedded.name == view.name)
        {
            embedded.query = view.query;
        } else if let Some(mut stored) = cassie.catalog.get_view(&view.name) {
            stored.query = view.query;
            propagation.views.push(stored);
        }
    }
    cassie
        .midge
        .publish_enum_group(next, renamed.then_some(current.name.as_str()), &propagation)
        .map_err(QueryError::from)?;
    if renamed {
        cassie.catalog.unregister_enum_type(&current.name);
    }
    cassie.catalog.register_enum_type(next.clone());
    cassie.catalog.apply_enum_propagation(propagation);
    Ok(())
}

/// Refuses `DROP TYPE` while a table column or a view holds the type (`2BP01`),
/// or with `CASCADE` (`0A000`: dropping dependents is not implemented).
pub(super) fn ensure_droppable(
    cassie: &Cassie,
    metadata: &EnumTypeMeta,
    cascade: bool,
) -> Result<(), QueryError> {
    let mut dependents = cassie.catalog.enum_type_dependents(metadata.type_oid());
    // A view that only casts to the type inside its definition has no typed
    // output column; its definition is the dependency record.
    let from_definitions =
        super::enum_views::definition_dependents(cassie, None, metadata, &dependents);
    dependents.extend(from_definitions);
    if dependents.is_empty() {
        return Ok(());
    }
    if cascade {
        return Err(diagnostic(
            "0A000",
            format!(
                "DROP TYPE ... CASCADE of \"{}\" with dependent objects is not supported",
                metadata.local_name()
            ),
        ));
    }
    let name = metadata.local_name();
    Err(CassieError::from(
        DiagnosticError::new(
            "2BP01",
            format!("cannot drop type {name} because other objects depend on it"),
        )
        .with_detail(
            dependents
                .iter()
                .map(|dependent| dependent.detail(&name))
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .with_hint("Use DROP ... CASCADE to drop the dependent objects too."),
    )
    .into())
}

/// Deletes the persisted descriptor and unregisters the type.
pub(super) fn remove_enum(
    cassie: &Cassie,
    database: &str,
    metadata: &EnumTypeMeta,
) -> Result<(), QueryError> {
    cassie
        .midge
        .delete_enum_type(database, &metadata.name)
        .map_err(QueryError::from)?;
    cassie.catalog.unregister_enum_type(&metadata.name);
    Ok(())
}

#[cfg(test)]
#[path = "enum_command_tests.rs"]
mod tests;
