use super::{normalize_new_relation_name, parsed_statement, resolve_existing_name, BindingContext};
use crate::app::{CassieError, CatalogObjectKind};
use crate::catalog::Catalog;
use crate::sql::ast::{ParsedStatement, QueryStatement};

pub(super) fn bind_create_materialized_projection_statement(
    mut statement: crate::sql::ast::CreateMaterializedProjectionStatement,
    catalog: &Catalog,
    raw_sql: &str,
    context: &BindingContext,
) -> Result<ParsedStatement, CassieError> {
    statement.name = normalize_new_relation_name(statement.name.trim(), context, catalog)?;
    if statement.name.is_empty() {
        return Err(CassieError::Planner(
            "CREATE MATERIALIZED PROJECTION requires a name".into(),
        ));
    }
    if !(statement.if_not_exists && catalog.is_materialized_projection(&statement.name)) {
        crate::sql::definition_guard::query_sql(&statement.query)?;
    }
    Ok(parsed_statement(
        raw_sql,
        QueryStatement::CreateMaterializedProjection(statement),
    ))
}

pub(super) fn bind_refresh_materialized_projection_statement(
    mut statement: crate::sql::ast::RefreshMaterializedProjectionStatement,
    catalog: &Catalog,
    raw_sql: &str,
    context: &BindingContext,
) -> Result<ParsedStatement, CassieError> {
    statement.name = resolve_existing_name(statement.name.trim(), context, |name| {
        catalog.is_materialized_projection(name)
    })?;
    if statement.name.is_empty() {
        return Err(CassieError::Planner(
            "REFRESH MATERIALIZED PROJECTION requires a name".into(),
        ));
    }
    Ok(parsed_statement(
        raw_sql,
        QueryStatement::RefreshMaterializedProjection(statement),
    ))
}

pub(super) fn bind_drop_materialized_projection_statement(
    mut statement: crate::sql::ast::DropMaterializedProjectionStatement,
    catalog: &Catalog,
    raw_sql: &str,
    context: &BindingContext,
) -> Result<ParsedStatement, CassieError> {
    statement.name = resolve_existing_name(statement.name.trim(), context, |name| {
        catalog.is_materialized_projection(name)
    })?;
    if statement.name.is_empty() {
        return Err(CassieError::Planner(
            "DROP MATERIALIZED PROJECTION requires a name".into(),
        ));
    }
    Ok(parsed_statement(
        raw_sql,
        QueryStatement::DropMaterializedProjection(statement),
    ))
}

pub(super) fn bind_alter_materialized_projection_statement(
    mut statement: crate::sql::ast::AlterMaterializedProjectionStatement,
    catalog: &Catalog,
    raw_sql: &str,
    context: &BindingContext,
) -> Result<ParsedStatement, CassieError> {
    statement.name = resolve_existing_name(statement.name.trim(), context, |name| {
        catalog.is_materialized_projection(name)
    })?;
    if statement.name.is_empty() {
        return Err(CassieError::Planner(
            "ALTER MATERIALIZED PROJECTION requires a name".into(),
        ));
    }
    Ok(parsed_statement(
        raw_sql,
        QueryStatement::AlterMaterializedProjection(statement),
    ))
}

pub(super) fn bind_drop_materialized_projection_version_statement(
    mut statement: crate::sql::ast::DropMaterializedProjectionVersionStatement,
    catalog: &Catalog,
    raw_sql: &str,
    context: &BindingContext,
) -> Result<ParsedStatement, CassieError> {
    statement.name = resolve_existing_name(statement.name.trim(), context, |name| {
        catalog.is_materialized_projection(name)
    })?;
    if statement.name.is_empty() {
        return Err(CassieError::Planner(
            "DROP MATERIALIZED PROJECTION VERSION requires a name".into(),
        ));
    }
    Ok(parsed_statement(
        raw_sql,
        QueryStatement::DropMaterializedProjectionVersion(statement),
    ))
}

pub(super) fn bind_verify_projection_statement(
    mut statement: crate::sql::ast::VerifyProjectionStatement,
    catalog: &Catalog,
    raw_sql: &str,
    context: &BindingContext,
) -> Result<ParsedStatement, CassieError> {
    statement.name = resolve_existing_name(statement.name.trim(), context, |name| {
        catalog.is_materialized_projection(name)
    })?;
    if statement.name.is_empty() {
        return Err(CassieError::Planner(
            "VERIFY PROJECTION requires a name".into(),
        ));
    }
    ensure_projection_target_exists(&statement.name, statement.version_id.as_deref(), catalog)?;
    Ok(parsed_statement(
        raw_sql,
        QueryStatement::VerifyProjection(statement),
    ))
}

pub(super) fn bind_diff_projection_statement(
    mut statement: crate::sql::ast::DiffProjectionStatement,
    catalog: &Catalog,
    raw_sql: &str,
    context: &BindingContext,
) -> Result<ParsedStatement, CassieError> {
    statement.left = normalize_projection_target(statement.left, catalog, context)?;
    statement.right = normalize_projection_target(statement.right, catalog, context)?;
    Ok(parsed_statement(
        raw_sql,
        QueryStatement::DiffProjection(statement),
    ))
}

pub(super) fn bind_compare_projection_statement(
    mut statement: crate::sql::ast::CompareProjectionStatement,
    catalog: &Catalog,
    raw_sql: &str,
    context: &BindingContext,
) -> Result<ParsedStatement, CassieError> {
    statement.target = normalize_projection_target(statement.target, catalog, context)?;
    Ok(parsed_statement(
        raw_sql,
        QueryStatement::CompareProjection(statement),
    ))
}

pub(super) fn bind_plan_repair_projection_statement(
    mut statement: crate::sql::ast::PlanRepairProjectionStatement,
    catalog: &Catalog,
    raw_sql: &str,
    context: &BindingContext,
) -> Result<ParsedStatement, CassieError> {
    statement.target = normalize_projection_target(statement.target, catalog, context)?;
    Ok(parsed_statement(
        raw_sql,
        QueryStatement::PlanRepairProjection(statement),
    ))
}

pub(super) fn bind_repair_projection_statement(
    mut statement: crate::sql::ast::RepairProjectionStatement,
    catalog: &Catalog,
    raw_sql: &str,
    context: &BindingContext,
) -> Result<ParsedStatement, CassieError> {
    statement.target = normalize_projection_target(statement.target, catalog, context)?;
    Ok(parsed_statement(
        raw_sql,
        QueryStatement::RepairProjection(statement),
    ))
}

fn normalize_projection_target(
    mut target: crate::sql::ast::ProjectionDiffTarget,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<crate::sql::ast::ProjectionDiffTarget, CassieError> {
    target.name = resolve_existing_name(target.name.trim(), context, |name| {
        catalog.is_materialized_projection(name)
    })?;
    if target.name.is_empty() {
        return Err(CassieError::Planner(
            "projection targets require a name".into(),
        ));
    }
    ensure_projection_target_exists(&target.name, target.version_id.as_deref(), catalog)?;
    Ok(target)
}

fn ensure_projection_target_exists(
    name: &str,
    version_id: Option<&str>,
    catalog: &Catalog,
) -> Result<(), CassieError> {
    if let Some(projection) = catalog.get_materialized_projection(name) {
        let Some(version_id) = version_id else {
            return Ok(());
        };
        if projection
            .versions
            .iter()
            .any(|version| version.version_id == version_id)
        {
            return Ok(());
        }
        return Err(CassieError::CatalogObjectNotFound {
            kind: CatalogObjectKind::ProjectionVersion,
            name: format!("{name} VERSION {version_id}"),
        });
    }
    if catalog.relation_exists(name) || catalog.get_projection_metadata(name).is_some() {
        return Ok(());
    }
    Err(CassieError::CatalogObjectNotFound {
        kind: CatalogObjectKind::Relation,
        name: name.to_string(),
    })
}
