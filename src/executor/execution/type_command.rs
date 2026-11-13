//! `CREATE TYPE ... AS (...)`, `DROP TYPE` and the `ALTER TYPE` dispatch (#828).

use super::command_output::empty as empty_command;
use super::{Cassie, CassieSession, QueryError, QueryResult};
use crate::app::{CassieError, DiagnosticError};
use crate::catalog::{
    canonical_relation_name, canonical_schema_name, is_system_schema, parse_name,
    CompositeTypeMeta, ParsedName, COMPOSITE_TYPE_FORMAT_VERSION, DEFAULT_SCHEMA,
};
use crate::runtime::QueryExecutionControls;
use crate::sql::ast::{TypeAction, TypeAttribute, TypeStatement};
use crate::types::{CompositeField, DataType};

pub(super) const ROOT_OID: u32 = 10;
/// First revision of a newly created composite descriptor.
const INITIAL_REVISION: u64 = 1;

pub(super) fn execute(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    statement: &TypeStatement,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let database = session
        .and_then(CassieSession::current_database)
        .unwrap_or(&cassie.default_database)
        .to_owned();
    let context = cassie.binding_context_for_session(session);
    match &statement.action {
        TypeAction::Create { name, attributes } => create(
            cassie,
            &database,
            context.current_schema(),
            name,
            attributes,
            controls,
        ),
        TypeAction::Alter { name, alteration } => super::type_alter::alter(
            cassie,
            &super::type_alter::AlterScope {
                database: &database,
                search_order: &context.relation_search_order(),
            },
            name,
            alteration,
            controls,
        ),
        TypeAction::Drop {
            names,
            if_exists,
            cascade,
        } => drop_types(
            cassie,
            &database,
            &context.relation_search_order(),
            names,
            *if_exists,
            *cascade,
            controls,
        ),
    }
}

pub(super) fn diagnostic(state: &'static str, message: String) -> QueryError {
    CassieError::from(DiagnosticError::new(state, message)).into()
}

/// Splits a requested type name into its schema and unqualified name.
fn creation_target(
    database: &str,
    current_schema: &str,
    raw: &str,
) -> Result<(String, String), QueryError> {
    match parse_name(raw).map_err(|message| diagnostic("42602", message))? {
        ParsedName::Unqualified(name) => Ok((current_schema.to_owned(), name)),
        ParsedName::SchemaQualified { schema, name } => Ok((schema, name)),
        ParsedName::DatabaseQualified {
            database: named,
            schema,
            name,
        } => {
            if named.eq_ignore_ascii_case(database) {
                Ok((schema, name))
            } else {
                Err(diagnostic(
                    "0A000",
                    "cross-database type references are not implemented".into(),
                ))
            }
        }
    }
}

fn create(
    cassie: &Cassie,
    database: &str,
    current_schema: &str,
    raw_name: &str,
    attributes: &[TypeAttribute],
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
    if relation_name_occupied(cassie, database, &canonical, controls)? {
        return Err(diagnostic(
            "42710",
            format!("type \"{name}\" already exists"),
        ));
    }
    let fields = validated_fields(attributes)?;
    let output = empty_command("CREATE TYPE", controls)?;
    let allocate = || cassie.midge.allocate_object_id().map_err(QueryError::from);
    let metadata = CompositeTypeMeta {
        format_version: COMPOSITE_TYPE_FORMAT_VERSION,
        database: database.to_owned(),
        name: canonical,
        object_id: allocate()?,
        array_object_id: allocate()?,
        relation_object_id: allocate()?,
        owner_oid: ROOT_OID,
        revision: INITIAL_REVISION,
        fields,
        dropped_positions: Vec::new(),
    };
    cassie
        .midge
        .put_composite_type(&metadata)
        .map_err(QueryError::from)?;
    cassie.catalog.register_composite_type(metadata);
    Ok(output)
}

/// A composite type owns a pg_class row, so it shares the relation namespace
/// with tables, views, sequences and indexes.
pub(super) fn relation_name_occupied(
    cassie: &Cassie,
    database: &str,
    canonical: &str,
    controls: &QueryExecutionControls,
) -> Result<bool, QueryError> {
    Ok(cassie.catalog.composite_type_exists(canonical)
        || cassie.catalog.get_schema(canonical).is_some()
        || cassie.catalog.relation_exists(canonical)
        || cassie.catalog.sequence_exists(canonical)
        || cassie.catalog.index_relation_name_occupied(
            canonical,
            database,
            false,
            Some(controls),
        )?)
}

/// Arrays of composite types are not stored yet.
pub(super) fn reject_composite_array(data_type: &DataType) -> Result<(), QueryError> {
    if matches!(data_type, DataType::Array(inner) if matches!(**inner, DataType::Composite(_))) {
        return Err(diagnostic(
            "0A000",
            "arrays of composite types are not supported".into(),
        ));
    }
    Ok(())
}

fn validated_fields(attributes: &[TypeAttribute]) -> Result<Vec<CompositeField>, QueryError> {
    let mut fields: Vec<CompositeField> = Vec::with_capacity(attributes.len());
    for attribute in attributes {
        if fields
            .iter()
            .any(|field| field.name.eq_ignore_ascii_case(&attribute.name))
        {
            return Err(diagnostic(
                "42701",
                format!("column \"{}\" specified more than once", attribute.name),
            ));
        }
        reject_composite_array(&attribute.data_type)?;
        fields.push(CompositeField {
            name: attribute.name.clone(),
            data_type: attribute.data_type.clone(),
        });
    }
    Ok(fields)
}

fn drop_types(
    cassie: &Cassie,
    database: &str,
    search_order: &[&str],
    names: &[String],
    if_exists: bool,
    cascade: bool,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let mut targets = Vec::with_capacity(names.len());
    for raw in names {
        let Some(metadata) = cassie
            .catalog
            .resolve_composite_type(database, search_order, raw)
        else {
            if if_exists {
                continue;
            }
            return Err(diagnostic(
                "42704",
                format!("type \"{raw}\" does not exist"),
            ));
        };
        let oid = metadata.type_oid();
        let dependents = cassie.catalog.composite_type_dependents(oid);
        if !dependents.is_empty() {
            if cascade {
                return Err(diagnostic(
                    "0A000",
                    format!(
                        "DROP TYPE ... CASCADE of \"{}\" with dependent objects is not supported",
                        metadata.local_name()
                    ),
                ));
            }
            return Err(CassieError::from(
                DiagnosticError::new(
                    "2BP01",
                    format!(
                        "cannot drop type {} because other objects depend on it",
                        metadata.local_name()
                    ),
                )
                .with_detail(
                    dependents
                        .iter()
                        .map(|dependent| {
                            format!("{dependent} depends on type {}", metadata.local_name())
                        })
                        .collect::<Vec<_>>()
                        .join("\n"),
                )
                .with_hint("Use DROP ... CASCADE to drop the dependent objects too."),
            )
            .into());
        }
        targets.push(metadata);
    }
    let output = empty_command("DROP TYPE", controls)?;
    for metadata in targets {
        cassie
            .midge
            .delete_composite_type(database, &metadata.name)
            .map_err(QueryError::from)?;
        cassie.catalog.unregister_composite_type(&metadata.name);
    }
    Ok(output)
}
