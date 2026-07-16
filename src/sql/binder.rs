use std::collections::{HashMap, HashSet};
use std::mem;

use crate::app::{CassieError, CatalogObjectKind};
use crate::catalog::{
    is_reserved_namespace, local_name, virtual_views, Catalog, CollectionSchema, IndexMeta,
};
use crate::embeddings::DistanceMetric;
use crate::search::bm25;
use crate::sql::ast::{
    AlterSchemaOperation, AlterSchemaStatement, AlterTableOperation, AlterTableStatement, BinaryOp,
    CallProcedureStatement, CatalogStatement, CommonTableExpression, CopyStatement,
    CreateDatabaseStatement, CreateFunctionStatement, CreateProcedureStatement,
    CreateSchemaStatement, CreateSequenceStatement, CreateViewStatement, CteQuery,
    DropDatabaseStatement, DropFunctionStatement, DropIndexStatement, DropProcedureStatement,
    DropSchemaStatement, DropSequenceStatement, DropViewStatement, Expr, FunctionCall,
    InsertSource, OrderExpr, ParsedStatement, ProjectionStatement, QuerySource, QueryStatement,
    RuntimeStatement, SelectItem, SelectSet, SelectStatement, StatementRoute,
};
use crate::types::{DataType, FieldSchema, Schema};

type CteScope = HashMap<String, Vec<String>>;

#[path = "binder/commands.rs"]
mod commands;
#[path = "binder/context.rs"]
mod context;
#[path = "binder/inference.rs"]
mod inference;
#[path = "binder/recursive.rs"]
mod recursive;
#[path = "binder/routines.rs"]
mod routines;
#[path = "binder/schema.rs"]
mod schema;
#[path = "binder/schema_sequences.rs"]
mod schema_sequences;
#[path = "binder/select.rs"]
mod select;
#[path = "binder/validation.rs"]
mod validation;

use commands::{
    bind_alter_retention_policy, bind_create_retention_policy, bind_create_rollup, bind_delete,
    bind_enforce_retention_policy, bind_insert, bind_update,
};
pub(crate) use context::normalize_relation_name;
pub use context::BindingContext;
use context::{
    normalize_database_name, normalize_schema_name, resolve_relation_name, resolve_schema_name,
};
pub(crate) use inference::infer_expr_type;
pub use inference::infer_select_schema;
use recursive::bind_recursive_cte_query;
use routines::{
    bind_call_procedure, bind_create_function, bind_create_procedure, bind_drop_function,
    bind_drop_procedure,
};
use schema::{
    bind_alter_schema, bind_alter_table, bind_create_graph, bind_create_index, bind_create_table,
    bind_create_view, bind_drop_index, bind_drop_schema, bind_drop_table, bind_drop_view,
};
use schema_sequences::{bind_create_sequence, bind_drop_sequence};
use select::bind_select;
use validation::{
    collect_expr, collect_item, collect_projection_aliases, qualified_fields,
    recursive_cte_reference_count, recursive_cte_references_self, select_contains_parameters,
    validate_distinct_on_order_prefix, validate_expression, validate_expression_operand_families,
    validate_expression_references, validate_function_calls, validate_functions,
    validate_order_by_references, validate_projection_references, validate_select_operand_families,
};

#[derive(Debug, Clone)]
pub struct BoundStatement {
    pub statement: ParsedStatement,
    pub indexes: Vec<IndexMeta>,
}

/// # Errors
///
/// Returns an error when validation, storage, or execution fails.
pub fn bind(statement: ParsedStatement, catalog: &Catalog) -> Result<BoundStatement, CassieError> {
    bind_with_context(statement, catalog, &BindingContext::default())
}

/// # Errors
///
/// Returns an error when validation, storage, or execution fails.
pub fn bind_with_context(
    statement: ParsedStatement,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<BoundStatement, CassieError> {
    let statement = bind_statement(statement, catalog, &HashMap::new(), context)?;
    let indexes = bound_indexes(&statement, catalog);
    Ok(BoundStatement { statement, indexes })
}

fn bound_indexes(statement: &ParsedStatement, catalog: &Catalog) -> Vec<IndexMeta> {
    let Some(collection) = bound_statement_collection(statement) else {
        return Vec::new();
    };
    catalog.list_indexes(&collection)
}

fn bound_statement_collection(statement: &ParsedStatement) -> Option<String> {
    match &statement.statement {
        QueryStatement::Select(select) => source_collection(&select.source),
        QueryStatement::Explain(statement) => bound_statement_collection(&statement.statement),
        _ => None,
    }
}

fn source_collection(source: &QuerySource) -> Option<String> {
    match source {
        QuerySource::Collection(collection) => Some(collection.clone()),
        QuerySource::Subquery { select, .. } => source_collection(&select.source),
        QuerySource::Join { left, .. } => source_collection(left),
        QuerySource::Cte(_) | QuerySource::TableFunction { .. } | QuerySource::SingleRow => None,
    }
}

fn bind_statement(
    statement: ParsedStatement,
    catalog: &Catalog,
    outer_scope: &CteScope,
    context: &BindingContext,
) -> Result<ParsedStatement, CassieError> {
    let raw_sql = statement.raw_sql.clone();
    match statement.statement.into_route() {
        StatementRoute::Runtime(statement) => {
            bind_runtime_route(statement, catalog, outer_scope, &raw_sql, context)
        }
        StatementRoute::Catalog(statement) => {
            bind_catalog_route(statement, catalog, &raw_sql, context)
        }
        StatementRoute::Projection(statement) => {
            bind_projection_route(statement, catalog, &raw_sql, context)
        }
        StatementRoute::Retention(statement) => {
            bind_retention_route(statement, catalog, &raw_sql, context)
        }
    }
}

fn bind_runtime_route(
    statement: RuntimeStatement,
    catalog: &Catalog,
    outer_scope: &CteScope,
    raw_sql: &str,
    context: &BindingContext,
) -> Result<ParsedStatement, CassieError> {
    match statement {
        RuntimeStatement::Select(select) => {
            bind_select_statement(select, catalog, outer_scope, raw_sql, context)
        }
        RuntimeStatement::Explain(statement) => {
            bind_explain_statement(statement, catalog, outer_scope, raw_sql, context)
        }
        RuntimeStatement::Show(statement) => Ok(bind_show_statement(statement, raw_sql)),
        RuntimeStatement::Set(statement) => Ok(bind_set_statement(statement, raw_sql)),
        RuntimeStatement::Copy(statement) => bind_catalog_statement(
            raw_sql,
            bind_copy(statement, catalog, context),
            QueryStatement::Copy,
        ),
        RuntimeStatement::Insert(statement) => bind_catalog_statement(
            raw_sql,
            bind_insert(statement, catalog, context),
            QueryStatement::Insert,
        ),
        RuntimeStatement::Update(statement) => bind_catalog_statement(
            raw_sql,
            bind_update(statement, catalog, context),
            QueryStatement::Update,
        ),
        RuntimeStatement::Delete(statement) => bind_catalog_statement(
            raw_sql,
            bind_delete(statement, catalog, context),
            QueryStatement::Delete,
        ),
        RuntimeStatement::Transaction(statement) => Ok(parsed_statement(
            raw_sql,
            QueryStatement::Transaction(statement),
        )),
    }
}

fn bind_catalog_route(
    statement: CatalogStatement,
    catalog: &Catalog,
    raw_sql: &str,
    context: &BindingContext,
) -> Result<ParsedStatement, CassieError> {
    match statement {
        CatalogStatement::CreateTable(statement) => bind_catalog_statement(
            raw_sql,
            bind_create_table(statement, catalog, context),
            QueryStatement::CreateTable,
        ),
        CatalogStatement::CreateGraph(statement) => bind_catalog_statement(
            raw_sql,
            bind_create_graph(statement, catalog, context),
            QueryStatement::CreateGraph,
        ),
        CatalogStatement::DropTable(statement) => bind_catalog_statement(
            raw_sql,
            bind_drop_table(statement, catalog, context),
            QueryStatement::DropTable,
        ),
        CatalogStatement::AlterTable(statement) => bind_catalog_statement(
            raw_sql,
            bind_alter_table(statement, catalog, context),
            QueryStatement::AlterTable,
        ),
        CatalogStatement::CreateSequence(statement) => bind_catalog_statement(
            raw_sql,
            bind_create_sequence(statement, catalog, context),
            QueryStatement::CreateSequence,
        ),
        CatalogStatement::DropSequence(statement) => bind_catalog_statement(
            raw_sql,
            bind_drop_sequence(statement, catalog, context),
            QueryStatement::DropSequence,
        ),
        CatalogStatement::CreateDatabase(statement) => {
            bind_create_database_statement(statement, catalog, raw_sql)
        }
        CatalogStatement::DropDatabase(statement) => {
            bind_drop_database_statement(statement, catalog, raw_sql)
        }
        CatalogStatement::CreateSchema(statement) => {
            bind_create_schema_statement(statement, catalog, raw_sql, context)
        }
        CatalogStatement::DropSchema(statement) => bind_catalog_statement(
            raw_sql,
            bind_drop_schema(statement, catalog, context),
            QueryStatement::DropSchema,
        ),
        CatalogStatement::AlterSchema(statement) => bind_catalog_statement(
            raw_sql,
            bind_alter_schema(statement, catalog, context),
            QueryStatement::AlterSchema,
        ),
        CatalogStatement::CreateView(statement) => bind_catalog_statement(
            raw_sql,
            bind_create_view(statement, catalog, context),
            QueryStatement::CreateView,
        ),
        CatalogStatement::DropView(statement) => bind_catalog_statement(
            raw_sql,
            bind_drop_view(statement, catalog, context),
            QueryStatement::DropView,
        ),
        CatalogStatement::CreateRole(statement) => Ok(parsed_statement(
            raw_sql,
            QueryStatement::CreateRole(statement),
        )),
        CatalogStatement::AlterRole(statement) => Ok(parsed_statement(
            raw_sql,
            QueryStatement::AlterRole(statement),
        )),
        CatalogStatement::DropRole(statement) => Ok(parsed_statement(
            raw_sql,
            QueryStatement::DropRole(statement),
        )),
        CatalogStatement::CreateFunction(statement) => {
            bind_catalog_routine_or_index_route(raw_sql, catalog, statement.into(), context)
        }
        CatalogStatement::DropFunction(statement) => {
            bind_catalog_routine_or_index_route(raw_sql, catalog, statement.into(), context)
        }
        CatalogStatement::CreateProcedure(statement) => {
            bind_catalog_routine_or_index_route(raw_sql, catalog, statement.into(), context)
        }
        CatalogStatement::DropProcedure(statement) => {
            bind_catalog_routine_or_index_route(raw_sql, catalog, statement.into(), context)
        }
        CatalogStatement::CallProcedure(statement) => {
            bind_catalog_routine_or_index_route(raw_sql, catalog, statement.into(), context)
        }
        CatalogStatement::CreateIndex(statement) => {
            bind_catalog_routine_or_index_route(raw_sql, catalog, statement.into(), context)
        }
        CatalogStatement::DropIndex(statement) => {
            bind_catalog_routine_or_index_route(raw_sql, catalog, statement.into(), context)
        }
    }
}

fn bind_catalog_routine_or_index_route(
    raw_sql: &str,
    catalog: &Catalog,
    statement: CatalogRoutineOrIndexStatement,
    context: &BindingContext,
) -> Result<ParsedStatement, CassieError> {
    match statement {
        CatalogRoutineOrIndexStatement::CreateFunction(statement) => bind_catalog_statement(
            raw_sql,
            bind_create_function(statement, catalog, context),
            QueryStatement::CreateFunction,
        ),
        CatalogRoutineOrIndexStatement::DropFunction(statement) => bind_catalog_statement(
            raw_sql,
            bind_drop_function(statement, catalog, context),
            QueryStatement::DropFunction,
        ),
        CatalogRoutineOrIndexStatement::CreateProcedure(statement) => bind_catalog_statement(
            raw_sql,
            bind_create_procedure(statement, catalog, context),
            QueryStatement::CreateProcedure,
        ),
        CatalogRoutineOrIndexStatement::DropProcedure(statement) => bind_catalog_statement(
            raw_sql,
            bind_drop_procedure(statement, catalog, context),
            QueryStatement::DropProcedure,
        ),
        CatalogRoutineOrIndexStatement::CallProcedure(statement) => bind_catalog_statement(
            raw_sql,
            bind_call_procedure(statement, catalog, context),
            QueryStatement::CallProcedure,
        ),
        CatalogRoutineOrIndexStatement::CreateIndex(statement) => bind_catalog_statement(
            raw_sql,
            bind_create_index(statement, catalog, context),
            QueryStatement::CreateIndex,
        ),
        CatalogRoutineOrIndexStatement::DropIndex(statement) => bind_catalog_statement(
            raw_sql,
            bind_drop_index(statement, catalog, context),
            QueryStatement::DropIndex,
        ),
    }
}

enum CatalogRoutineOrIndexStatement {
    CreateFunction(CreateFunctionStatement),
    DropFunction(DropFunctionStatement),
    CreateProcedure(CreateProcedureStatement),
    DropProcedure(DropProcedureStatement),
    CallProcedure(CallProcedureStatement),
    CreateIndex(crate::sql::ast::CreateIndexStatement),
    DropIndex(DropIndexStatement),
}

impl From<CreateFunctionStatement> for CatalogRoutineOrIndexStatement {
    fn from(statement: CreateFunctionStatement) -> Self {
        Self::CreateFunction(statement)
    }
}

impl From<DropFunctionStatement> for CatalogRoutineOrIndexStatement {
    fn from(statement: DropFunctionStatement) -> Self {
        Self::DropFunction(statement)
    }
}

impl From<CreateProcedureStatement> for CatalogRoutineOrIndexStatement {
    fn from(statement: CreateProcedureStatement) -> Self {
        Self::CreateProcedure(statement)
    }
}

impl From<DropProcedureStatement> for CatalogRoutineOrIndexStatement {
    fn from(statement: DropProcedureStatement) -> Self {
        Self::DropProcedure(statement)
    }
}

impl From<CallProcedureStatement> for CatalogRoutineOrIndexStatement {
    fn from(statement: CallProcedureStatement) -> Self {
        Self::CallProcedure(statement)
    }
}

impl From<crate::sql::ast::CreateIndexStatement> for CatalogRoutineOrIndexStatement {
    fn from(statement: crate::sql::ast::CreateIndexStatement) -> Self {
        Self::CreateIndex(statement)
    }
}

impl From<DropIndexStatement> for CatalogRoutineOrIndexStatement {
    fn from(statement: DropIndexStatement) -> Self {
        Self::DropIndex(statement)
    }
}

fn bind_projection_route(
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
            bind_create_materialized_projection_statement(statement, raw_sql, context)
        }
        ProjectionStatement::RefreshMaterializedProjection(statement) => {
            bind_refresh_materialized_projection_statement(statement, raw_sql, context)
        }
        ProjectionStatement::DropMaterializedProjection(statement) => {
            bind_drop_materialized_projection_statement(statement, raw_sql, context)
        }
        ProjectionStatement::AlterMaterializedProjection(statement) => {
            bind_alter_materialized_projection_statement(statement, raw_sql, context)
        }
        ProjectionStatement::DropMaterializedProjectionVersion(statement) => {
            bind_drop_materialized_projection_version_statement(statement, raw_sql, context)
        }
        ProjectionStatement::VerifyProjection(statement) => {
            bind_verify_projection_statement(statement, raw_sql, context)
        }
        ProjectionStatement::DiffProjection(statement) => {
            bind_diff_projection_statement(statement, raw_sql, context)
        }
        ProjectionStatement::CompareProjection(statement) => {
            bind_compare_projection_statement(statement, raw_sql, context)
        }
        ProjectionStatement::PlanRepairProjection(statement) => {
            bind_plan_repair_projection_statement(statement, raw_sql, context)
        }
        ProjectionStatement::RepairProjection(statement) => {
            bind_repair_projection_statement(statement, raw_sql, context)
        }
    }
}

fn bind_retention_route(
    statement: crate::sql::ast::RetentionStatement,
    catalog: &Catalog,
    raw_sql: &str,
    context: &BindingContext,
) -> Result<ParsedStatement, CassieError> {
    match statement {
        crate::sql::ast::RetentionStatement::CreateRetentionPolicy(statement) => {
            bind_catalog_statement(
                raw_sql,
                bind_create_retention_policy(statement, catalog, context),
                QueryStatement::CreateRetentionPolicy,
            )
        }
        crate::sql::ast::RetentionStatement::AlterRetentionPolicy(statement) => {
            bind_catalog_statement(
                raw_sql,
                bind_alter_retention_policy(statement, catalog, context),
                QueryStatement::AlterRetentionPolicy,
            )
        }
        crate::sql::ast::RetentionStatement::DropRetentionPolicy(statement) => {
            bind_drop_retention_policy_statement(statement, catalog, raw_sql, context)
        }
        crate::sql::ast::RetentionStatement::EnforceRetentionPolicy(statement) => {
            bind_catalog_statement(
                raw_sql,
                bind_enforce_retention_policy(statement, catalog, context),
                QueryStatement::EnforceRetentionPolicy,
            )
        }
    }
}

fn bind_select_statement(
    select: SelectStatement,
    catalog: &Catalog,
    outer_scope: &CteScope,
    raw_sql: &str,
    context: &BindingContext,
) -> Result<ParsedStatement, CassieError> {
    let select = bind_select(select, catalog, outer_scope, context)?;
    Ok(parsed_statement(raw_sql, QueryStatement::Select(select)))
}

fn bind_explain_statement(
    statement: crate::sql::ast::ExplainStatement,
    catalog: &Catalog,
    outer_scope: &CteScope,
    raw_sql: &str,
    context: &BindingContext,
) -> Result<ParsedStatement, CassieError> {
    let inner = bind_statement(*statement.statement, catalog, outer_scope, context)?;
    Ok(parsed_statement(
        raw_sql,
        QueryStatement::Explain(crate::sql::ast::ExplainStatement {
            analyze: statement.analyze,
            statement: Box::new(inner),
        }),
    ))
}

fn bind_show_statement(
    statement: crate::sql::ast::ShowStatement,
    raw_sql: &str,
) -> ParsedStatement {
    let mut statement = statement;
    statement.variable = statement.variable.trim().to_string();
    parsed_statement(raw_sql, QueryStatement::Show(statement))
}

fn bind_set_statement(statement: crate::sql::ast::SetStatement, raw_sql: &str) -> ParsedStatement {
    let mut statement = statement;
    statement.variable = statement.variable.trim().to_string();
    statement.value = statement.value.map(|value| value.trim().to_string());
    parsed_statement(raw_sql, QueryStatement::Set(statement))
}

fn bind_refresh_rollup_statement(
    statement: crate::sql::ast::RefreshRollupStatement,
    catalog: &Catalog,
    raw_sql: &str,
    context: &BindingContext,
) -> Result<ParsedStatement, CassieError> {
    let crate::sql::ast::RefreshRollupStatement { name } = statement;
    let name = normalize_relation_name(name.trim(), context)?;
    if name.is_empty() {
        return Err(CassieError::Planner(
            "REFRESH ROLLUP requires a name".into(),
        ));
    }
    if catalog.get_rollup(&name).is_none() {
        return Err(CassieError::CatalogObjectNotFound {
            kind: CatalogObjectKind::Rollup,
            name,
        });
    }
    Ok(parsed_statement(
        raw_sql,
        QueryStatement::RefreshRollup(crate::sql::ast::RefreshRollupStatement { name }),
    ))
}

fn bind_drop_rollup_statement(
    statement: crate::sql::ast::DropRollupStatement,
    catalog: &Catalog,
    raw_sql: &str,
    context: &BindingContext,
) -> Result<ParsedStatement, CassieError> {
    let crate::sql::ast::DropRollupStatement { name, if_exists } = statement;
    let name = normalize_relation_name(name.trim(), context)?;
    if name.is_empty() {
        return Err(CassieError::Planner("DROP ROLLUP requires a name".into()));
    }
    if !if_exists && catalog.get_rollup(&name).is_none() {
        return Err(CassieError::CatalogObjectNotFound {
            kind: CatalogObjectKind::Rollup,
            name,
        });
    }
    Ok(parsed_statement(
        raw_sql,
        QueryStatement::DropRollup(crate::sql::ast::DropRollupStatement { name, if_exists }),
    ))
}

fn bind_create_materialized_projection_statement(
    mut statement: crate::sql::ast::CreateMaterializedProjectionStatement,
    raw_sql: &str,
    context: &BindingContext,
) -> Result<ParsedStatement, CassieError> {
    statement.name = normalize_relation_name(statement.name.trim(), context)?;
    if statement.name.is_empty() {
        return Err(CassieError::Planner(
            "CREATE MATERIALIZED PROJECTION requires a name".into(),
        ));
    }
    Ok(parsed_statement(
        raw_sql,
        QueryStatement::CreateMaterializedProjection(statement),
    ))
}

fn bind_refresh_materialized_projection_statement(
    mut statement: crate::sql::ast::RefreshMaterializedProjectionStatement,
    raw_sql: &str,
    context: &BindingContext,
) -> Result<ParsedStatement, CassieError> {
    statement.name = normalize_relation_name(statement.name.trim(), context)?;
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

fn bind_drop_materialized_projection_statement(
    mut statement: crate::sql::ast::DropMaterializedProjectionStatement,
    raw_sql: &str,
    context: &BindingContext,
) -> Result<ParsedStatement, CassieError> {
    statement.name = normalize_relation_name(statement.name.trim(), context)?;
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

fn bind_alter_materialized_projection_statement(
    mut statement: crate::sql::ast::AlterMaterializedProjectionStatement,
    raw_sql: &str,
    context: &BindingContext,
) -> Result<ParsedStatement, CassieError> {
    statement.name = normalize_relation_name(statement.name.trim(), context)?;
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

fn bind_drop_materialized_projection_version_statement(
    mut statement: crate::sql::ast::DropMaterializedProjectionVersionStatement,
    raw_sql: &str,
    context: &BindingContext,
) -> Result<ParsedStatement, CassieError> {
    statement.name = normalize_relation_name(statement.name.trim(), context)?;
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

fn bind_verify_projection_statement(
    mut statement: crate::sql::ast::VerifyProjectionStatement,
    raw_sql: &str,
    context: &BindingContext,
) -> Result<ParsedStatement, CassieError> {
    statement.name = normalize_relation_name(statement.name.trim(), context)?;
    if statement.name.is_empty() {
        return Err(CassieError::Planner(
            "VERIFY PROJECTION requires a name".into(),
        ));
    }
    Ok(parsed_statement(
        raw_sql,
        QueryStatement::VerifyProjection(statement),
    ))
}

fn bind_diff_projection_statement(
    mut statement: crate::sql::ast::DiffProjectionStatement,
    raw_sql: &str,
    context: &BindingContext,
) -> Result<ParsedStatement, CassieError> {
    statement.left = normalize_projection_target(statement.left, context)?;
    statement.right = normalize_projection_target(statement.right, context)?;
    Ok(parsed_statement(
        raw_sql,
        QueryStatement::DiffProjection(statement),
    ))
}

fn bind_compare_projection_statement(
    mut statement: crate::sql::ast::CompareProjectionStatement,
    raw_sql: &str,
    context: &BindingContext,
) -> Result<ParsedStatement, CassieError> {
    statement.target = normalize_projection_target(statement.target, context)?;
    Ok(parsed_statement(
        raw_sql,
        QueryStatement::CompareProjection(statement),
    ))
}

fn bind_plan_repair_projection_statement(
    mut statement: crate::sql::ast::PlanRepairProjectionStatement,
    raw_sql: &str,
    context: &BindingContext,
) -> Result<ParsedStatement, CassieError> {
    statement.target = normalize_projection_target(statement.target, context)?;
    Ok(parsed_statement(
        raw_sql,
        QueryStatement::PlanRepairProjection(statement),
    ))
}

fn bind_repair_projection_statement(
    mut statement: crate::sql::ast::RepairProjectionStatement,
    raw_sql: &str,
    context: &BindingContext,
) -> Result<ParsedStatement, CassieError> {
    statement.target = normalize_projection_target(statement.target, context)?;
    Ok(parsed_statement(
        raw_sql,
        QueryStatement::RepairProjection(statement),
    ))
}

fn normalize_projection_target(
    mut target: crate::sql::ast::ProjectionDiffTarget,
    context: &BindingContext,
) -> Result<crate::sql::ast::ProjectionDiffTarget, CassieError> {
    target.name = normalize_relation_name(target.name.trim(), context)?;
    if target.name.is_empty() {
        return Err(CassieError::Planner(
            "projection targets require a name".into(),
        ));
    }
    Ok(target)
}

fn bind_drop_retention_policy_statement(
    statement: crate::sql::ast::DropRetentionPolicyStatement,
    catalog: &Catalog,
    raw_sql: &str,
    context: &BindingContext,
) -> Result<ParsedStatement, CassieError> {
    let crate::sql::ast::DropRetentionPolicyStatement { name, if_exists } = statement;
    let name = normalize_relation_name(name.trim(), context)?;
    if name.is_empty() {
        return Err(CassieError::Planner(
            "DROP RETENTION POLICY requires a name".into(),
        ));
    }
    if !if_exists && catalog.get_retention_policy(&name).is_none() {
        return Err(CassieError::CatalogObjectNotFound {
            kind: CatalogObjectKind::RetentionPolicy,
            name,
        });
    }
    Ok(parsed_statement(
        raw_sql,
        QueryStatement::DropRetentionPolicy(crate::sql::ast::DropRetentionPolicyStatement {
            name,
            if_exists,
        }),
    ))
}

fn bind_create_schema_statement(
    statement: CreateSchemaStatement,
    catalog: &Catalog,
    raw_sql: &str,
    context: &BindingContext,
) -> Result<ParsedStatement, CassieError> {
    let CreateSchemaStatement {
        schema,
        if_not_exists,
    } = statement;
    let schema = normalize_schema_name(schema.trim(), context)?;
    if schema.is_empty() {
        return Err(CassieError::Planner("CREATE SCHEMA requires a name".into()));
    }
    if is_reserved_namespace(&local_name(&schema)) {
        return Err(CassieError::Unsupported(format!(
            "namespace '{schema}' is reserved"
        )));
    }
    if !if_not_exists && catalog.namespace_exists(&schema) {
        return Err(CassieError::Planner(format!(
            "namespace '{schema}' already exists"
        )));
    }

    Ok(parsed_statement(
        raw_sql,
        QueryStatement::CreateSchema(CreateSchemaStatement {
            schema,
            if_not_exists,
        }),
    ))
}

fn bind_create_database_statement(
    statement: CreateDatabaseStatement,
    catalog: &Catalog,
    raw_sql: &str,
) -> Result<ParsedStatement, CassieError> {
    let CreateDatabaseStatement {
        name,
        if_not_exists,
    } = statement;
    let name = normalize_database_name(name.trim())?;
    if name.is_empty() {
        return Err(CassieError::Planner(
            "CREATE DATABASE requires a name".into(),
        ));
    }
    if !if_not_exists && catalog.database_exists(&name) {
        return Err(CassieError::Planner(format!(
            "database '{name}' already exists"
        )));
    }
    Ok(parsed_statement(
        raw_sql,
        QueryStatement::CreateDatabase(CreateDatabaseStatement {
            name,
            if_not_exists,
        }),
    ))
}

fn bind_drop_database_statement(
    statement: DropDatabaseStatement,
    catalog: &Catalog,
    raw_sql: &str,
) -> Result<ParsedStatement, CassieError> {
    let DropDatabaseStatement { name, if_exists } = statement;
    let name = normalize_database_name(name.trim())?;
    if name.is_empty() {
        return Err(CassieError::Planner("DROP DATABASE requires a name".into()));
    }
    if !if_exists && !catalog.database_exists(&name) {
        return Err(CassieError::CatalogObjectNotFound {
            kind: CatalogObjectKind::Database,
            name,
        });
    }
    Ok(parsed_statement(
        raw_sql,
        QueryStatement::DropDatabase(DropDatabaseStatement { name, if_exists }),
    ))
}

fn parsed_statement(raw_sql: &str, statement: QueryStatement) -> ParsedStatement {
    ParsedStatement {
        raw_sql: raw_sql.to_string(),
        statement,
    }
}

fn bind_catalog_statement<T>(
    raw_sql: &str,
    statement: Result<T, CassieError>,
    bind: impl FnOnce(T) -> QueryStatement,
) -> Result<ParsedStatement, CassieError> {
    statement.map(|statement| parsed_statement(raw_sql, bind(statement)))
}

fn bind_copy(
    mut statement: CopyStatement,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<CopyStatement, CassieError> {
    statement.table = resolve_relation_name(statement.table.trim(), catalog, context)?;
    if statement.table.is_empty() {
        return Err(CassieError::Planner("COPY requires a target table".into()));
    }
    let schema = catalog.get_schema(&statement.table).ok_or_else(|| {
        CassieError::Planner(format!("collection '{}' not found", statement.table))
    })?;

    if statement.columns.is_empty() {
        return Ok(statement);
    }

    for column in &mut statement.columns {
        *column = column.trim().to_string();
        if column.is_empty() {
            return Err(CassieError::Planner(
                "COPY column list cannot include empty columns".into(),
            ));
        }
        if column.eq_ignore_ascii_case("_id") {
            continue;
        }
        if schema
            .fields
            .iter()
            .any(|field| field.name.eq_ignore_ascii_case(column))
        {
            continue;
        }
        if column.eq_ignore_ascii_case("id")
            && !schema
                .fields
                .iter()
                .any(|field| field.name.eq_ignore_ascii_case("id"))
        {
            continue;
        }
        return Err(CassieError::Planner(format!(
            "COPY target column '{column}' does not exist in '{}'",
            statement.table
        )));
    }

    Ok(statement)
}
