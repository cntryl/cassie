use std::collections::{HashMap, HashSet};
use std::mem;

use crate::app::CassieError;
use crate::catalog::{is_reserved_namespace, virtual_views, Catalog, CollectionSchema, IndexMeta};
use crate::embeddings::DistanceMetric;
use crate::search::bm25;
use crate::sql::ast::{
    AlterSchemaOperation, AlterSchemaStatement, AlterTableOperation, AlterTableStatement,
    CallProcedureStatement, CommonTableExpression, CopyStatement, CreateFunctionStatement,
    CreateProcedureStatement, CreateSchemaStatement, CreateSequenceStatement, CreateViewStatement,
    CteQuery, DropFunctionStatement, DropIndexStatement, DropProcedureStatement,
    DropSchemaStatement, DropSequenceStatement, DropViewStatement, Expr, FunctionCall,
    InsertSource, OrderExpr, ParsedStatement, QuerySource, QueryStatement, SelectItem, SelectSet,
    SelectStatement,
};
use crate::types::{DataType, FieldSchema, Schema};

type CteScope = HashMap<String, Vec<String>>;

#[path = "binder/commands.rs"]
mod commands;
#[path = "binder/inference.rs"]
mod inference;
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
pub use inference::infer_select_schema;
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
    collect_item, collect_projection_aliases, qualified_fields, recursive_cte_references_self,
    select_contains_parameters, validate_distinct_on_order_prefix, validate_expression,
    validate_expression_references, validate_function_calls, validate_functions,
    validate_order_by_references, validate_projection_references,
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
    let statement = bind_statement(statement, catalog, &HashMap::new())?;
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
) -> Result<ParsedStatement, CassieError> {
    let raw_sql = statement.raw_sql.clone();
    match statement.statement {
        QueryStatement::Select(select) => {
            bind_select_statement(select, catalog, outer_scope, &raw_sql)
        }
        QueryStatement::Explain(statement) => {
            bind_explain_statement(statement, catalog, outer_scope, &raw_sql)
        }
        other => bind_non_select_statement(other, catalog, &raw_sql),
    }
}

fn bind_non_select_statement(
    statement: QueryStatement,
    catalog: &Catalog,
    raw_sql: &str,
) -> Result<ParsedStatement, CassieError> {
    match statement {
        QueryStatement::Show(statement) => Ok(bind_show_statement(statement, raw_sql)),
        QueryStatement::Set(statement) => Ok(bind_set_statement(statement, raw_sql)),
        QueryStatement::Copy(statement) => {
            bind_catalog_statement(raw_sql, bind_copy(statement, catalog), QueryStatement::Copy)
        }
        QueryStatement::CreateTable(statement) => bind_catalog_statement(
            raw_sql,
            bind_create_table(statement, catalog),
            QueryStatement::CreateTable,
        ),
        QueryStatement::CreateGraph(statement) => bind_catalog_statement(
            raw_sql,
            bind_create_graph(statement, catalog),
            QueryStatement::CreateGraph,
        ),
        QueryStatement::DropTable(statement) => bind_catalog_statement(
            raw_sql,
            bind_drop_table(statement, catalog),
            QueryStatement::DropTable,
        ),
        QueryStatement::AlterTable(statement) => bind_catalog_statement(
            raw_sql,
            bind_alter_table(statement, catalog),
            QueryStatement::AlterTable,
        ),
        QueryStatement::CreateSequence(statement) => bind_catalog_statement(
            raw_sql,
            bind_create_sequence(statement, catalog),
            QueryStatement::CreateSequence,
        ),
        QueryStatement::DropSequence(statement) => bind_catalog_statement(
            raw_sql,
            bind_drop_sequence(statement, catalog),
            QueryStatement::DropSequence,
        ),
        QueryStatement::CreateIndex(statement) => bind_catalog_statement(
            raw_sql,
            bind_create_index(statement, catalog),
            QueryStatement::CreateIndex,
        ),
        QueryStatement::DropIndex(statement) => bind_catalog_statement(
            raw_sql,
            bind_drop_index(statement, catalog),
            QueryStatement::DropIndex,
        ),
        QueryStatement::CreateSchema(statement) => {
            bind_create_schema_statement(statement, catalog, raw_sql)
        }
        QueryStatement::DropSchema(statement) => bind_catalog_statement(
            raw_sql,
            bind_drop_schema(statement, catalog),
            QueryStatement::DropSchema,
        ),
        QueryStatement::AlterSchema(statement) => bind_catalog_statement(
            raw_sql,
            bind_alter_schema(statement, catalog),
            QueryStatement::AlterSchema,
        ),
        QueryStatement::CreateView(statement) => bind_catalog_statement(
            raw_sql,
            bind_create_view(statement, catalog),
            QueryStatement::CreateView,
        ),
        QueryStatement::DropView(statement) => bind_catalog_statement(
            raw_sql,
            bind_drop_view(statement, catalog),
            QueryStatement::DropView,
        ),
        other => bind_runtime_statement(other, catalog, raw_sql),
    }
}

fn bind_runtime_statement(
    statement: QueryStatement,
    catalog: &Catalog,
    raw_sql: &str,
) -> Result<ParsedStatement, CassieError> {
    match statement {
        QueryStatement::CreateRollup(statement) => bind_catalog_statement(
            raw_sql,
            bind_create_rollup(statement, catalog),
            QueryStatement::CreateRollup,
        ),
        QueryStatement::RefreshRollup(statement) => {
            bind_refresh_rollup_statement(statement, catalog, raw_sql)
        }
        QueryStatement::DropRollup(statement) => {
            bind_drop_rollup_statement(statement, catalog, raw_sql)
        }
        QueryStatement::CreateMaterializedProjection(statement) => Ok(parsed_statement(
            raw_sql,
            QueryStatement::CreateMaterializedProjection(statement),
        )),
        QueryStatement::RefreshMaterializedProjection(statement) => Ok(parsed_statement(
            raw_sql,
            QueryStatement::RefreshMaterializedProjection(statement),
        )),
        QueryStatement::DropMaterializedProjection(statement) => Ok(parsed_statement(
            raw_sql,
            QueryStatement::DropMaterializedProjection(statement),
        )),
        QueryStatement::AlterMaterializedProjection(statement) => Ok(parsed_statement(
            raw_sql,
            QueryStatement::AlterMaterializedProjection(statement),
        )),
        QueryStatement::DropMaterializedProjectionVersion(statement) => Ok(parsed_statement(
            raw_sql,
            QueryStatement::DropMaterializedProjectionVersion(statement),
        )),
        QueryStatement::VerifyProjection(statement) => Ok(parsed_statement(
            raw_sql,
            QueryStatement::VerifyProjection(statement),
        )),
        QueryStatement::DiffProjection(statement) => Ok(parsed_statement(
            raw_sql,
            QueryStatement::DiffProjection(statement),
        )),
        QueryStatement::CompareProjection(statement) => Ok(parsed_statement(
            raw_sql,
            QueryStatement::CompareProjection(statement),
        )),
        QueryStatement::PlanRepairProjection(statement) => Ok(parsed_statement(
            raw_sql,
            QueryStatement::PlanRepairProjection(statement),
        )),
        QueryStatement::RepairProjection(statement) => Ok(parsed_statement(
            raw_sql,
            QueryStatement::RepairProjection(statement),
        )),
        QueryStatement::CreateRetentionPolicy(statement) => bind_catalog_statement(
            raw_sql,
            bind_create_retention_policy(statement, catalog),
            QueryStatement::CreateRetentionPolicy,
        ),
        QueryStatement::AlterRetentionPolicy(statement) => bind_catalog_statement(
            raw_sql,
            bind_alter_retention_policy(statement, catalog),
            QueryStatement::AlterRetentionPolicy,
        ),
        QueryStatement::DropRetentionPolicy(statement) => {
            bind_drop_retention_policy_statement(statement, catalog, raw_sql)
        }
        QueryStatement::EnforceRetentionPolicy(statement) => bind_catalog_statement(
            raw_sql,
            bind_enforce_retention_policy(statement, catalog),
            QueryStatement::EnforceRetentionPolicy,
        ),
        other => bind_program_statement(other, catalog, raw_sql),
    }
}

fn bind_program_statement(
    statement: QueryStatement,
    catalog: &Catalog,
    raw_sql: &str,
) -> Result<ParsedStatement, CassieError> {
    match statement {
        QueryStatement::CreateRole(statement) => Ok(parsed_statement(
            raw_sql,
            QueryStatement::CreateRole(statement),
        )),
        QueryStatement::AlterRole(statement) => Ok(parsed_statement(
            raw_sql,
            QueryStatement::AlterRole(statement),
        )),
        QueryStatement::DropRole(statement) => Ok(parsed_statement(
            raw_sql,
            QueryStatement::DropRole(statement),
        )),
        QueryStatement::CreateFunction(statement) => bind_catalog_statement(
            raw_sql,
            bind_create_function(statement, catalog),
            QueryStatement::CreateFunction,
        ),
        QueryStatement::DropFunction(statement) => bind_catalog_statement(
            raw_sql,
            bind_drop_function(statement, catalog),
            QueryStatement::DropFunction,
        ),
        QueryStatement::CreateProcedure(statement) => bind_catalog_statement(
            raw_sql,
            bind_create_procedure(statement, catalog),
            QueryStatement::CreateProcedure,
        ),
        QueryStatement::DropProcedure(statement) => bind_catalog_statement(
            raw_sql,
            bind_drop_procedure(statement, catalog),
            QueryStatement::DropProcedure,
        ),
        QueryStatement::CallProcedure(statement) => bind_catalog_statement(
            raw_sql,
            bind_call_procedure(statement, catalog),
            QueryStatement::CallProcedure,
        ),
        QueryStatement::Insert(statement) => bind_catalog_statement(
            raw_sql,
            bind_insert(statement, catalog),
            QueryStatement::Insert,
        ),
        QueryStatement::Update(statement) => bind_catalog_statement(
            raw_sql,
            bind_update(statement, catalog),
            QueryStatement::Update,
        ),
        QueryStatement::Delete(statement) => bind_catalog_statement(
            raw_sql,
            bind_delete(statement, catalog),
            QueryStatement::Delete,
        ),
        QueryStatement::Transaction(statement) => Ok(parsed_statement(
            raw_sql,
            QueryStatement::Transaction(statement),
        )),
        _ => unreachable!(),
    }
}

fn bind_select_statement(
    select: SelectStatement,
    catalog: &Catalog,
    outer_scope: &CteScope,
    raw_sql: &str,
) -> Result<ParsedStatement, CassieError> {
    let select = bind_select(select, catalog, outer_scope)?;
    Ok(parsed_statement(raw_sql, QueryStatement::Select(select)))
}

fn bind_explain_statement(
    statement: crate::sql::ast::ExplainStatement,
    catalog: &Catalog,
    outer_scope: &CteScope,
    raw_sql: &str,
) -> Result<ParsedStatement, CassieError> {
    let inner = bind_statement(*statement.statement, catalog, outer_scope)?;
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
) -> Result<ParsedStatement, CassieError> {
    let crate::sql::ast::RefreshRollupStatement { name } = statement;
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(CassieError::Planner(
            "REFRESH ROLLUP requires a name".into(),
        ));
    }
    if catalog.get_rollup(&name).is_none() {
        return Err(CassieError::Planner(format!(
            "rollup '{name}' does not exist"
        )));
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
) -> Result<ParsedStatement, CassieError> {
    let crate::sql::ast::DropRollupStatement { name, if_exists } = statement;
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(CassieError::Planner("DROP ROLLUP requires a name".into()));
    }
    if !if_exists && catalog.get_rollup(&name).is_none() {
        return Err(CassieError::Planner(format!(
            "rollup '{name}' does not exist"
        )));
    }
    Ok(parsed_statement(
        raw_sql,
        QueryStatement::DropRollup(crate::sql::ast::DropRollupStatement { name, if_exists }),
    ))
}

fn bind_drop_retention_policy_statement(
    statement: crate::sql::ast::DropRetentionPolicyStatement,
    catalog: &Catalog,
    raw_sql: &str,
) -> Result<ParsedStatement, CassieError> {
    let crate::sql::ast::DropRetentionPolicyStatement { name, if_exists } = statement;
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(CassieError::Planner(
            "DROP RETENTION POLICY requires a name".into(),
        ));
    }
    if !if_exists && catalog.get_retention_policy(&name).is_none() {
        return Err(CassieError::Planner(format!(
            "retention policy '{name}' does not exist"
        )));
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
) -> Result<ParsedStatement, CassieError> {
    let CreateSchemaStatement {
        schema,
        if_not_exists,
    } = statement;
    let schema = schema.trim().to_string();
    if schema.is_empty() {
        return Err(CassieError::Planner("CREATE SCHEMA requires a name".into()));
    }
    if is_reserved_namespace(&schema) {
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
) -> Result<CopyStatement, CassieError> {
    statement.table = statement.table.trim().to_string();
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
