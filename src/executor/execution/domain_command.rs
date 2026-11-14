//! `CREATE DOMAIN`, `ALTER DOMAIN` and `DROP DOMAIN` (#828).

use super::command_output::empty as empty_command;
use super::{Cassie, CassieSession, QueryError, QueryResult};
use crate::app::{CassieError, DiagnosticError, DomainRowRule};
use crate::catalog::{
    canonical_relation_name, canonical_schema_name, is_system_schema, parse_name,
    same_stored_relation, DomainTypeMeta, ParsedName, DEFAULT_SCHEMA, DOMAIN_TYPE_FORMAT_VERSION,
};
use crate::runtime::QueryExecutionControls;
use crate::sql::ast::{
    DomainAction, DomainAlteration, DomainCheckDefinition, DomainDefaultDefinition,
};
use crate::types::domain::{DOMAIN_VALUE_COLUMN, MAX_DOMAIN_CHECKS};
use crate::types::{DataType, DomainCheck, DomainDefault};

const ROOT_OID: u32 = 10;
/// First revision of a newly created domain descriptor.
const INITIAL_REVISION: u64 = 1;

pub(super) fn execute(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    action: &DomainAction,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let database = session
        .and_then(CassieSession::current_database)
        .unwrap_or(&cassie.default_database)
        .to_owned();
    let context = cassie.binding_context_for_session(session);
    match action {
        DomainAction::Create { .. } => create(
            cassie,
            &database,
            context.current_schema(),
            action,
            controls,
        ),
        DomainAction::Alter { name, alteration } => alter(
            cassie,
            session,
            &database,
            &context.relation_search_order(),
            name,
            alteration,
            controls,
        ),
        DomainAction::Drop {
            names,
            if_exists,
            cascade,
        } => drop_domains(
            cassie,
            &database,
            &context.relation_search_order(),
            names,
            (*if_exists, *cascade),
            controls,
        ),
    }
}

fn diagnostic(state: &'static str, message: String) -> QueryError {
    CassieError::from(DiagnosticError::new(state, message)).into()
}

/// Splits a requested domain name into its schema and unqualified name.
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

/// A domain shares the type namespace with composite types and with the row
/// types of tables and views. Sequence and index names do not collide with it
/// (PostgreSQL accepts a domain named like a sequence or an index).
pub(super) fn type_name_taken(cassie: &Cassie, canonical: &str) -> bool {
    cassie.catalog.domain_type_exists(canonical)
        || cassie.catalog.enum_type_exists(canonical)
        || cassie.catalog.composite_type_exists(canonical)
        || cassie.catalog.get_schema(canonical).is_some()
        || cassie.catalog.relation_exists(canonical)
        || relation_owns_type_name(cassie, canonical)
}

/// Whether a table or view in the same schema (and database, when both names
/// are canonical) has the local name `canonical`. A stored relation name may
/// be bare while the requested domain name is canonical, which the suffix
/// matching in `relation_exists` cannot see.
fn relation_owns_type_name(cassie: &Cassie, canonical: &str) -> bool {
    cassie
        .catalog
        .list_collections_canonical()
        .iter()
        .any(|collection| same_stored_relation(&collection.name, canonical))
        || cassie
            .catalog
            .list_views()
            .iter()
            .any(|view| same_stored_relation(&view.name, canonical))
}

fn validate_base(base: &DataType) -> Result<(), QueryError> {
    let unsupported = |what: &str| {
        Err(diagnostic(
            "0A000",
            format!("domains over {what} are not supported in this version"),
        ))
    };
    match base {
        DataType::Composite(_) => unsupported("composite types"),
        DataType::Enum(_) => unsupported("enum types"),
        DataType::Domain(_) => unsupported("domains"),
        DataType::JsonPath | DataType::Null => unsupported("pseudo-types"),
        DataType::Array(inner)
            if matches!(
                **inner,
                DataType::Composite(_) | DataType::Domain(_) | DataType::Enum(_)
            ) =>
        {
            unsupported("arrays of composite types or domains")
        }
        DataType::Array(inner) if matches!(**inner, DataType::Enum(_)) => {
            unsupported("arrays of enum types")
        }
        _ => Ok(()),
    }
}

fn bind_default(
    base: &DataType,
    definition: Option<&DomainDefaultDefinition>,
) -> Result<Option<DomainDefault>, QueryError> {
    let Some(definition) = definition else {
        return Ok(None);
    };
    if let Some(expression) = &definition.expression {
        if !crate::catalog::volatile_default_accepts(expression, base) {
            return Err(diagnostic(
                "42804",
                format!(
                    "default {expression} cannot be used for a domain over {}",
                    base.type_name()
                ),
            ));
        }
        return Ok(Some(DomainDefault {
            sql: definition.sql.clone(),
            value: None,
            expression: Some(expression.clone()),
        }));
    }
    match &definition.value {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(value) => {
            // A numeric literal keeps its written scale (`1.50`), which the
            // parsed JSON number has already lost.
            let literal =
                super::domain_check_definition::exact_numeric_default(base, &definition.sql)
                    .map_or_else(|| value.clone(), serde_json::Value::String);
            let coerced = crate::catalog::coerce_default_literal(base, literal)
                .map_err(|message| diagnostic("42804", message))?;
            Ok(Some(DomainDefault {
                sql: definition.sql.clone(),
                value: Some(coerced),
                expression: None,
            }))
        }
    }
}

/// Rejects what PostgreSQL rejects in a domain CHECK: column references,
/// subqueries and parameters.
fn validate_check_expression(sql: &str, base: &DataType) -> Result<(), QueryError> {
    use crate::sql::ast::Expr;

    let expression = crate::sql::parser::parse_expression(sql)
        .map_err(|error| diagnostic("42601", error.to_string()))?;
    let mut problem: Option<(&'static str, &'static str)> = None;
    let flagged = expression.any_descendant_or_self(&mut |node| {
        let found = match node {
            Expr::Column(name) if !name.eq_ignore_ascii_case(DOMAIN_VALUE_COLUMN) => Some((
                "0A000",
                "cannot use column references in domain CHECK constraint",
            )),
            Expr::Subquery(_) | Expr::Exists(_) => {
                Some(("0A000", "cannot use subquery in check constraint"))
            }
            Expr::Param(_) => Some((
                "42P02",
                "there is no parameter in a domain CHECK constraint",
            )),
            Expr::Function(function)
                if crate::sql::functions::is_aggregate_function(&function.name) =>
            {
                Some((
                    "42803",
                    "aggregate functions are not allowed in check constraints",
                ))
            }
            _ => None,
        };
        if found.is_some() {
            problem = found;
        }
        found.is_some()
    });
    match problem {
        Some((state, message)) if flagged => Err(diagnostic(state, message.to_owned())),
        _ => super::domain_check_definition::validate(&expression, sql, base),
    }
}

/// Appends `definitions` to `checks`, naming unnamed constraints as
/// PostgreSQL does (`domain_check`, `domain_check1`, ...).
fn add_checks(
    domain: &str,
    base: &DataType,
    checks: &mut Vec<DomainCheck>,
    definitions: &[DomainCheckDefinition],
    validated: bool,
) -> Result<(), QueryError> {
    for definition in definitions {
        if checks.len() >= MAX_DOMAIN_CHECKS {
            return Err(diagnostic(
                "54000",
                format!("a domain cannot have more than {MAX_DOMAIN_CHECKS} CHECK constraints"),
            ));
        }
        validate_check_expression(&definition.sql, base)?;
        let taken = |candidate: &str| checks.iter().any(|check| check.name == candidate);
        let name = if let Some(name) = &definition.name {
            if taken(name) {
                return Err(diagnostic(
                    "42710",
                    format!("constraint \"{name}\" for domain \"{domain}\" already exists"),
                ));
            }
            name.clone()
        } else {
            let mut candidate = format!("{domain}_check");
            let mut suffix = 0_u32;
            while taken(&candidate) {
                suffix += 1;
                candidate = format!("{domain}_check{suffix}");
            }
            candidate
        };
        checks.push(DomainCheck {
            name,
            sql: definition.sql.clone(),
            validated,
        });
    }
    Ok(())
}

fn create(
    cassie: &Cassie,
    database: &str,
    current_schema: &str,
    action: &DomainAction,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let DomainAction::Create {
        name: raw_name,
        base,
        default,
        not_null,
        checks: definitions,
    } = action
    else {
        return Err(diagnostic("XX000", "expected CREATE DOMAIN".into()));
    };
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
    validate_base(base)?;
    let default = bind_default(base, default.as_ref())?;
    let mut checks = Vec::with_capacity(definitions.len());
    add_checks(&name, base, &mut checks, definitions, true)?;
    let output = empty_command("CREATE DOMAIN", controls)?;
    let allocate = || cassie.midge.allocate_object_id().map_err(QueryError::from);
    let metadata = DomainTypeMeta {
        format_version: DOMAIN_TYPE_FORMAT_VERSION,
        database: database.to_owned(),
        name: canonical,
        object_id: allocate()?,
        array_object_id: allocate()?,
        owner_oid: ROOT_OID,
        revision: INITIAL_REVISION,
        base: base.clone(),
        not_null: *not_null,
        default,
        checks,
    };
    cassie
        .midge
        .put_domain_type(&metadata)
        .map_err(QueryError::from)?;
    cassie.catalog.register_domain_type(metadata);
    Ok(output)
}

/// Resolves `raw` to a domain, or reports PostgreSQL's error for a missing
/// type or a type that is not a domain.
fn resolve_domain(
    cassie: &Cassie,
    database: &str,
    search_order: &[&str],
    raw: &str,
) -> Result<Option<DomainTypeMeta>, QueryError> {
    if let Some(domain) = cassie
        .catalog
        .resolve_domain_type(database, search_order, raw)
    {
        return Ok(Some(domain));
    }
    if cassie
        .catalog
        .resolve_composite_type(database, search_order, raw)
        .is_some()
    {
        return Err(diagnostic("42809", format!("\"{raw}\" is not a domain")));
    }
    Ok(None)
}

fn publish(
    cassie: &Cassie,
    previous: &DomainTypeMeta,
    mut updated: DomainTypeMeta,
) -> Result<(), QueryError> {
    updated.revision = previous.revision.saturating_add(1);
    let renamed = !updated.name.eq_ignore_ascii_case(&previous.name);
    // A view stores its definition as text, so casts that name the domain
    // follow a rename or a move to another schema.
    let views = if renamed {
        super::domain_view_dependents::renamed_views(cassie, previous, &updated)
    } else {
        Vec::new()
    };
    cassie
        .midge
        .put_domain_type(&updated)
        .map_err(QueryError::from)?;
    if renamed {
        cassie
            .midge
            .delete_domain_type(&previous.database, &previous.name)
            .map_err(QueryError::from)?;
        cassie.catalog.unregister_domain_type(&previous.name);
    }
    cassie.catalog.register_domain_type(updated);
    super::domain_view_dependents::publish_views(cassie, views)
}

/// PostgreSQL cannot validate a constraint against values stored inside a
/// column of a row type that contains the domain, and refuses the change
/// (`0A000`) rather than publish a constraint those values may violate.
fn reject_row_type_containers(cassie: &Cassie, meta: &DomainTypeMeta) -> Result<(), QueryError> {
    match cassie
        .catalog
        .domain_container_columns(meta.type_oid())
        .first()
    {
        Some((collection, column)) => Err(diagnostic(
            "0A000",
            format!(
                "cannot alter type \"{}\" because column \"{}.{column}\" uses it",
                meta.local_name(),
                crate::catalog::local_name(collection)
            ),
        )),
        None => Ok(()),
    }
}

fn missing_constraint(domain: &str, name: &str) -> QueryError {
    diagnostic(
        "42704",
        format!("constraint \"{name}\" of domain \"{domain}\" does not exist"),
    )
}

/// The result of applying one `ALTER DOMAIN` operation.
enum Altered {
    Changed(Box<DomainTypeMeta>),
    Unchanged,
}

fn add_check(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    updated: &mut DomainTypeMeta,
    check: &DomainCheckDefinition,
    not_valid: bool,
) -> Result<(), QueryError> {
    let base = updated.base.clone();
    add_checks(
        &updated.local_name(),
        &base,
        &mut updated.checks,
        std::slice::from_ref(check),
        !not_valid,
    )?;
    if not_valid {
        return Ok(());
    }
    reject_row_type_containers(cassie, updated)?;
    let added = updated
        .checks
        .last()
        .cloned()
        .ok_or_else(|| diagnostic("XX000", "added domain constraint is missing".into()))?;
    cassie
        .validate_domain_rows(session, updated.type_oid(), &DomainRowRule::Check(&added))
        .map_err(QueryError::from)
}

fn validate_constraint(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    updated: &mut DomainTypeMeta,
    name: &str,
) -> Result<(), QueryError> {
    let local = updated.local_name();
    let oid = updated.type_oid();
    let Some(check) = updated.checks.iter_mut().find(|check| check.name == name) else {
        return Err(missing_constraint(&local, name));
    };
    if check.validated {
        return Ok(());
    }
    check.validated = true;
    let validated = check.clone();
    reject_row_type_containers(cassie, updated)?;
    cassie
        .validate_domain_rows(session, oid, &DomainRowRule::Check(&validated))
        .map_err(QueryError::from)
}

fn rename_constraint(updated: &mut DomainTypeMeta, from: &str, to: &str) -> Result<(), QueryError> {
    let local = updated.local_name();
    if updated.checks.iter().any(|check| check.name == to) {
        return Err(diagnostic(
            "42710",
            format!("constraint \"{to}\" for domain \"{local}\" already exists"),
        ));
    }
    let Some(check) = updated.checks.iter_mut().find(|check| check.name == from) else {
        return Err(missing_constraint(&local, from));
    };
    to.clone_into(&mut check.name);
    Ok(())
}

fn rename_domain(
    cassie: &Cassie,
    updated: &mut DomainTypeMeta,
    database: &str,
    new_name: &str,
) -> Result<(), QueryError> {
    let ParsedName::Unqualified(new_name) =
        parse_name(new_name).map_err(|message| diagnostic("42602", message))?
    else {
        return Err(diagnostic(
            "42601",
            "the new domain name cannot be qualified".into(),
        ));
    };
    let schema = updated.schema_name();
    let canonical = canonical_relation_name(database, &schema, &new_name);
    if type_name_taken(cassie, &canonical) {
        return Err(diagnostic(
            "42710",
            format!("type \"{new_name}\" already exists in schema \"{schema}\""),
        ));
    }
    updated.name = canonical;
    Ok(())
}

fn move_domain(
    cassie: &Cassie,
    updated: &mut DomainTypeMeta,
    database: &str,
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
            .namespace_exists(&canonical_schema_name(database, schema)))
    {
        return Err(diagnostic(
            "3F000",
            format!("schema \"{schema}\" does not exist"),
        ));
    }
    let local = updated.local_name();
    let canonical = canonical_relation_name(database, schema, &local);
    if type_name_taken(cassie, &canonical) {
        return Err(diagnostic(
            "42710",
            format!("type \"{local}\" already exists in schema \"{schema}\""),
        ));
    }
    updated.name = canonical;
    Ok(())
}

fn apply_alteration(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    meta: &DomainTypeMeta,
    database: &str,
    alteration: &DomainAlteration,
) -> Result<Altered, QueryError> {
    let local = meta.local_name();
    let mut updated = meta.clone();
    match alteration {
        DomainAlteration::SetDefault(definition) => {
            updated.default = bind_default(&meta.base, Some(definition))?;
        }
        DomainAlteration::DropDefault => updated.default = None,
        DomainAlteration::SetNotNull => {
            if !meta.not_null {
                reject_row_type_containers(cassie, meta)?;
                cassie
                    .validate_domain_rows(session, meta.type_oid(), &DomainRowRule::NotNull)
                    .map_err(QueryError::from)?;
            }
            updated.not_null = true;
        }
        DomainAlteration::DropNotNull => updated.not_null = false,
        DomainAlteration::AddCheck { check, not_valid } => {
            add_check(cassie, session, &mut updated, check, *not_valid)?;
        }
        DomainAlteration::DropConstraint { name, if_exists } => {
            let position = updated.checks.iter().position(|check| check.name == *name);
            match position {
                Some(position) => {
                    updated.checks.remove(position);
                }
                None if *if_exists => return Ok(Altered::Unchanged),
                None => return Err(missing_constraint(&local, name)),
            }
        }
        DomainAlteration::ValidateConstraint { name } => {
            validate_constraint(cassie, session, &mut updated, name)?;
        }
        DomainAlteration::RenameConstraint { from, to } => {
            rename_constraint(&mut updated, from, to)?;
        }
        DomainAlteration::RenameTo(new_name) => {
            rename_domain(cassie, &mut updated, database, new_name)?;
        }
        DomainAlteration::SetSchema(schema) => {
            if schema.eq_ignore_ascii_case(&meta.schema_name()) {
                return Ok(Altered::Unchanged);
            }
            move_domain(cassie, &mut updated, database, schema)?;
        }
    }
    Ok(Altered::Changed(Box::new(updated)))
}

/// Applies `alteration` to a domain named by `ALTER TYPE`, which reports the
/// `ALTER TYPE` command tag.
pub(super) fn alter_through_type(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    meta: &DomainTypeMeta,
    alteration: &DomainAlteration,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let altered = apply_alteration(cassie, session, meta, &meta.database, alteration)?;
    let output = empty_command("ALTER TYPE", controls)?;
    if let Altered::Changed(updated) = altered {
        publish(cassie, meta, *updated)?;
    }
    Ok(output)
}

fn alter(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    database: &str,
    search_order: &[&str],
    raw_name: &str,
    alteration: &DomainAlteration,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let Some(meta) = resolve_domain(cassie, database, search_order, raw_name)? else {
        return Err(diagnostic(
            "42704",
            format!("type \"{raw_name}\" does not exist"),
        ));
    };
    let altered = apply_alteration(cassie, session, &meta, database, alteration)?;
    let output = empty_command("ALTER DOMAIN", controls)?;
    if let Altered::Changed(updated) = altered {
        publish(cassie, &meta, *updated)?;
    }
    Ok(output)
}

/// Checks that nothing keeps `metadata` alive, as `DROP DOMAIN` and `DROP TYPE` do.
pub(super) fn ensure_droppable(
    cassie: &Cassie,
    metadata: &DomainTypeMeta,
    cascade: bool,
) -> Result<(), QueryError> {
    let mut dependents = cassie.catalog.domain_type_dependents(metadata.type_oid());
    if cascade && dependents.is_empty() {
        // Dependent views are removed with the domain by `remove_domain`.
        return Ok(());
    }
    if !cascade {
        dependents.extend(
            super::domain_view_dependents::direct_dependents(cassie, metadata)
                .iter()
                .map(|view| format!("view {}", crate::catalog::local_name(&view.name))),
        );
    }
    if dependents.is_empty() {
        return Ok(());
    }
    if cascade {
        return Err(diagnostic(
            "0A000",
            format!(
                "DROP DOMAIN ... CASCADE of \"{}\" with dependent objects is not supported",
                metadata.local_name()
            ),
        ));
    }
    Err(CassieError::from(
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
                .map(|dependent| format!("{dependent} depends on type {}", metadata.local_name()))
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .with_hint("Use DROP ... CASCADE to drop the dependent objects too."),
    )
    .into())
}

/// Removes a validated domain from storage and the catalog, with the views
/// that cast to it. `ensure_droppable` has already refused a drop without
/// `CASCADE` while such a view exists, so views are only removed by a cascade.
pub(super) fn remove_domain(
    cassie: &Cassie,
    database: &str,
    metadata: &DomainTypeMeta,
) -> Result<(), QueryError> {
    super::domain_view_dependents::drop_dependent_views(cassie, metadata)?;
    cassie
        .midge
        .delete_domain_type(database, &metadata.name)
        .map_err(QueryError::from)?;
    cassie.catalog.unregister_domain_type(&metadata.name);
    Ok(())
}

fn drop_domains(
    cassie: &Cassie,
    database: &str,
    search_order: &[&str],
    names: &[String],
    (if_exists, cascade): (bool, bool),
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let mut targets = Vec::with_capacity(names.len());
    for raw in names {
        let Some(metadata) = resolve_domain(cassie, database, search_order, raw)? else {
            if if_exists {
                continue;
            }
            return Err(diagnostic(
                "42704",
                format!("type \"{raw}\" does not exist"),
            ));
        };
        ensure_droppable(cassie, &metadata, cascade)?;
        targets.push(metadata);
    }
    let output = empty_command("DROP DOMAIN", controls)?;
    for metadata in targets {
        remove_domain(cassie, database, &metadata)?;
    }
    Ok(output)
}
