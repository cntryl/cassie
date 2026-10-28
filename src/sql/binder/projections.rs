use super::{
    bind_catalog_statement, bind_create_rollup, bind_drop_rollup_statement,
    bind_refresh_rollup_statement, normalize_new_relation_name, parsed_statement,
    resolve_existing_name, BindingContext,
};
use crate::app::{CassieError, CatalogObjectKind};
use crate::catalog::Catalog;
use crate::sql::ast::{ParsedStatement, ProjectionStatement, QueryStatement};

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

pub(super) fn bind_create_materialized_view_statement(
    statement: crate::sql::ast::CreateMaterializedViewStatement,
    catalog: &Catalog,
    raw_sql: &str,
    context: &BindingContext,
) -> Result<ParsedStatement, CassieError> {
    let projection = crate::sql::ast::CreateMaterializedProjectionStatement {
        name: statement.name,
        if_not_exists: statement.if_not_exists,
        options: std::collections::BTreeMap::new(),
        query: statement.query,
    };
    let bound =
        bind_create_materialized_projection_statement(projection, catalog, raw_sql, context)?;
    let QueryStatement::CreateMaterializedProjection(projection) = bound.statement else {
        unreachable!("projection binder returns its input statement family")
    };
    let statement = crate::sql::ast::CreateMaterializedViewStatement {
        name: projection.name,
        query: projection.query,
        if_not_exists: statement.if_not_exists,
        columns: statement.columns,
        with_data: statement.with_data,
    };
    Ok(parsed_statement(
        raw_sql,
        QueryStatement::CreateMaterializedView(statement),
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

pub(super) fn bind_refresh_materialized_view_statement(
    statement: crate::sql::ast::RefreshMaterializedViewStatement,
    catalog: &Catalog,
    raw_sql: &str,
    context: &BindingContext,
) -> Result<ParsedStatement, CassieError> {
    let crate::sql::ast::RefreshMaterializedViewStatement {
        name: raw_name,
        with_data,
    } = statement;
    let name = resolve_existing_name(raw_name.trim(), context, |name| {
        is_sql_materialized_view(catalog, name)
    })?;
    Ok(parsed_statement(
        raw_sql,
        QueryStatement::RefreshMaterializedView(
            crate::sql::ast::RefreshMaterializedViewStatement { name, with_data },
        ),
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

pub(super) fn bind_drop_materialized_view_statement(
    statement: crate::sql::ast::DropMaterializedViewStatement,
    catalog: &Catalog,
    raw_sql: &str,
    context: &BindingContext,
) -> Result<ParsedStatement, CassieError> {
    let crate::sql::ast::DropMaterializedViewStatement {
        name: raw_name,
        if_exists,
    } = statement;
    let name = resolve_existing_name(raw_name.trim(), context, |name| {
        is_sql_materialized_view(catalog, name)
    })?;
    Ok(parsed_statement(
        raw_sql,
        QueryStatement::DropMaterializedView(crate::sql::ast::DropMaterializedViewStatement {
            name,
            if_exists,
        }),
    ))
}

fn is_sql_materialized_view(catalog: &Catalog, name: &str) -> bool {
    catalog
        .get_materialized_projection(name)
        .is_some_and(|projection| {
            projection
                .materialized
                .as_ref()
                .is_some_and(crate::catalog::MaterializedProjectionMeta::is_sql_materialized_view)
        })
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

pub(super) fn bind_rename_materialized_view_statement(
    mut statement: crate::sql::ast::RenameMaterializedViewStatement,
    catalog: &Catalog,
    raw_sql: &str,
    context: &BindingContext,
) -> Result<ParsedStatement, CassieError> {
    statement.name = resolve_existing_name(&statement.name, context, |name| {
        catalog.relation_exists(name) || catalog.is_materialized_projection(name)
    })?;
    if !is_sql_materialized_view(catalog, &statement.name) {
        if catalog.relation_exists(&statement.name)
            || catalog.is_materialized_projection(&statement.name)
        {
            return Err(crate::app::DiagnosticError::new(
                "42809",
                format!("'{}' is not a materialized view", statement.name),
            )
            .into());
        }
        return Err(CassieError::CollectionNotFound(statement.name));
    }
    statement.name = catalog
        .get_materialized_projection(&statement.name)
        .ok_or_else(|| CassieError::CollectionNotFound(statement.name.clone()))?
        .collection;
    let crate::catalog::ParsedName::Unqualified(name) =
        crate::catalog::parse_name(&statement.new_name).map_err(CassieError::Planner)?
    else {
        return Err(CassieError::Planner(
            "RENAME TO requires an unqualified name".into(),
        ));
    };
    statement.new_name = match crate::catalog::parse_name(&statement.name)
        .map_err(CassieError::Planner)?
    {
        crate::catalog::ParsedName::DatabaseQualified {
            database, schema, ..
        } => crate::catalog::canonical_relation_name(&database, &schema, &name),
        crate::catalog::ParsedName::SchemaQualified { schema, .. } => normalize_new_relation_name(
            &crate::catalog::canonical_schema_name(&schema, &name),
            context,
            catalog,
        )?,
        crate::catalog::ParsedName::Unqualified(_) => {
            normalize_new_relation_name(&statement.new_name, context, catalog)?
        }
    };
    if catalog.relation_exists(&statement.new_name)
        || catalog
            .get_projection_metadata(&statement.new_name)
            .is_some()
    {
        return Err(crate::app::DiagnosticError::new(
            "42P07",
            format!("relation '{}' already exists", statement.new_name),
        )
        .into());
    }
    Ok(parsed_statement(
        raw_sql,
        QueryStatement::RenameMaterializedView(statement),
    ))
}

pub(super) fn bind_projection_route(
    statement: ProjectionStatement,
    catalog: &Catalog,
    raw_sql: &str,
    context: &BindingContext,
) -> Result<ParsedStatement, CassieError> {
    match statement {
        ProjectionStatement::CreateRollup(statement) => bind_catalog_statement(
            raw_sql,
            bind_create_rollup(statement, catalog, context),
            QueryStatement::CreateRollup,
        ),
        ProjectionStatement::RefreshRollup(statement) => {
            bind_refresh_rollup_statement(statement, catalog, raw_sql, context)
        }
        ProjectionStatement::DropRollup(statement) => {
            bind_drop_rollup_statement(statement, catalog, raw_sql, context)
        }
        ProjectionStatement::CreateMaterializedProjection(statement) => {
            bind_create_materialized_projection_statement(statement, catalog, raw_sql, context)
        }
        ProjectionStatement::RefreshMaterializedProjection(statement) => {
            bind_refresh_materialized_projection_statement(statement, catalog, raw_sql, context)
        }
        ProjectionStatement::DropMaterializedProjection(statement) => {
            bind_drop_materialized_projection_statement(statement, catalog, raw_sql, context)
        }
        ProjectionStatement::CreateMaterializedView(statement) => {
            bind_create_materialized_view_statement(statement, catalog, raw_sql, context)
        }
        ProjectionStatement::RefreshMaterializedView(statement) => {
            bind_refresh_materialized_view_statement(statement, catalog, raw_sql, context)
        }
        ProjectionStatement::DropMaterializedView(statement) => {
            bind_drop_materialized_view_statement(statement, catalog, raw_sql, context)
        }
        ProjectionStatement::RenameMaterializedView(statement) => {
            bind_rename_materialized_view_statement(statement, catalog, raw_sql, context)
        }
        ProjectionStatement::AlterMaterializedProjection(statement) => {
            bind_alter_materialized_projection_statement(statement, catalog, raw_sql, context)
        }
        ProjectionStatement::DropMaterializedProjectionVersion(statement) => {
            bind_drop_materialized_projection_version_statement(
                statement, catalog, raw_sql, context,
            )
        }
        ProjectionStatement::VerifyProjection(statement) => {
            bind_verify_projection_statement(statement, catalog, raw_sql, context)
        }
        ProjectionStatement::DiffProjection(statement) => {
            bind_diff_projection_statement(statement, catalog, raw_sql, context)
        }
        ProjectionStatement::CompareProjection(statement) => {
            bind_compare_projection_statement(statement, catalog, raw_sql, context)
        }
        ProjectionStatement::PlanRepairProjection(statement) => {
            bind_plan_repair_projection_statement(statement, catalog, raw_sql, context)
        }
        ProjectionStatement::RepairProjection(statement) => {
            bind_repair_projection_statement(statement, catalog, raw_sql, context)
        }
    }
}
