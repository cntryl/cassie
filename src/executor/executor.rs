use std::collections::{BTreeMap, HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::time::Instant;

use crate::app::{Cassie, CassieSession};
use crate::catalog;
use crate::catalog::virtual_views;
use crate::catalog::{CollectionSchema, FieldMeta, FunctionMeta, ProcedureMeta, Volatility};
use crate::embeddings::{DistanceMetric, VectorIndexMetadata, VectorIndexRecord};
use crate::executor::batch::{self, Batch, BatchRow, RowAccess};
use crate::executor::{aggregate, filter, projection, scan, sort};
use crate::planner::logical::{LogicalCommand, LogicalPlan};
use crate::planner::physical::PhysicalPlan;
use crate::runtime::QueryExecutionControls;
use crate::sql::ast::{
    CommonTableExpression, CteQuery, Expr, FunctionCall, InsertSource, JoinKind, QuerySource,
    SelectItem, SelectSet, SelectStatement, SetOperator,
};
use crate::types::{DataType, FieldSchema, Schema, Value};

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct ColumnMeta {
    pub name: String,
    pub data_type: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct QueryResult {
    pub columns: Vec<ColumnMeta>,
    pub rows: Vec<Vec<Value>>,
    pub command: String,
}

#[derive(Debug)]
pub enum QueryError {
    General(String),
}

type CteRows = Vec<Vec<(String, Value)>>;
type CteContext = HashMap<String, CteRows>;
type CteExecution<'a> = Pin<Box<dyn Future<Output = Result<CteRows, QueryError>> + Send + 'a>>;
type ExprResolution<'a> = Pin<Box<dyn Future<Output = Result<Expr, QueryError>> + Send + 'a>>;
type SourceExecution<'a> =
    Pin<Box<dyn Future<Output = Result<(Vec<Batch>, Vec<String>), QueryError>> + Send + 'a>>;

struct SourceExecutionEnv<'a> {
    cassie: &'a Cassie,
    session: Option<&'a CassieSession>,
    user_functions: &'a HashMap<String, FunctionMeta>,
    params: &'a [Value],
    controls: &'a QueryExecutionControls,
}

pub async fn run(
    cassie: &Cassie,
    plan: PhysicalPlan,
    params: Vec<Value>,
) -> Result<QueryResult, QueryError> {
    let controls = cassie.runtime.query_controls(std::time::Instant::now());
    run_with_controls(cassie, plan, params, &controls).await
}

pub async fn run_with_controls(
    cassie: &Cassie,
    plan: PhysicalPlan,
    params: Vec<Value>,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    run_with_session_controls(cassie, None, plan, params, controls).await
}

pub(crate) async fn run_with_session_controls(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    plan: PhysicalPlan,
    params: Vec<Value>,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let user_functions = cassie
        .catalog
        .list_functions()
        .await
        .into_iter()
        .map(|metadata| (metadata.name.to_ascii_lowercase(), metadata))
        .collect::<HashMap<String, FunctionMeta>>();

    if let Some(command) = plan.logical.command.as_ref() {
        return execute_command(cassie, session, command, &params, &user_functions, controls).await;
    }

    let mut cte_context: CteContext = HashMap::new();
    let rows = execute_plan(
        cassie,
        session,
        &plan.logical,
        &mut cte_context,
        &user_functions,
        &params,
        controls,
    )
    .await?;

    let columns = aggregate::columns_from_projection(&plan.logical.projection);
    let rows: Vec<Vec<Value>> = rows.into_iter().map(BatchRow::into_values).collect();

    if rows.len() > controls.max_result_rows {
        return Err(QueryError::General(format!(
            "query result row limit exceeded: {} > {}",
            rows.len(),
            controls.max_result_rows
        )));
    }

    Ok(QueryResult {
        columns,
        rows,
        command: "SELECT".to_string(),
    })
}

async fn execute_command(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    command: &LogicalCommand,
    params: &[Value],
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    check_timeout(controls)?;
    let mut invalidate_plan_cache = false;
    let result = match command {
        LogicalCommand::Show(statement) => {
            let variable = statement.variable.trim().to_ascii_lowercase();
            if variable.is_empty() {
                return Err(QueryError::General("SHOW requires a variable".to_string()));
            }

            match variable.as_str() {
                "search_path" => Ok(QueryResult {
                    columns: vec![ColumnMeta {
                        name: "search_path".to_string(),
                        data_type: "text".to_string(),
                    }],
                    rows: vec![vec![Value::String("public".to_string())]],
                    command: "SHOW".to_string(),
                }),
                "server_version" => Ok(QueryResult {
                    columns: vec![ColumnMeta {
                        name: "server_version".to_string(),
                        data_type: "text".to_string(),
                    }],
                    rows: vec![vec![Value::String(env!("CARGO_PKG_VERSION").to_string())]],
                    command: "SHOW".to_string(),
                }),
                _ => Err(QueryError::General(format!(
                    "unsupported SHOW variable '{}'",
                    statement.variable
                ))),
            }
        }
        LogicalCommand::Set(statement) => {
            let variable = statement.variable.trim().to_ascii_lowercase();
            if variable.is_empty() {
                return Err(QueryError::General("SET requires a variable".to_string()));
            }

            match variable.as_str() {
                "search_path" => {
                    let value = statement.value.as_deref().unwrap_or("").trim();
                    if value.is_empty() || value.eq_ignore_ascii_case("public") {
                        Ok(QueryResult {
                            columns: Vec::new(),
                            rows: Vec::new(),
                            command: "SET".to_string(),
                        })
                    } else {
                        Err(QueryError::General(format!(
                            "unsupported search_path value '{}' for SET",
                            value
                        )))
                    }
                }
                _ => Err(QueryError::General(format!(
                    "unsupported SET variable '{}', supported variables: search_path",
                    statement.variable
                ))),
            }
        }
        LogicalCommand::Insert(statement) => {
            execute_insert(cassie, session, statement, params, user_functions, controls).await
        }
        LogicalCommand::Update(statement) => {
            execute_update(cassie, session, statement, params, user_functions, controls).await
        }
        LogicalCommand::Delete(statement) => {
            execute_delete(cassie, session, statement, params, user_functions, controls).await
        }
        LogicalCommand::CreateTable(statement) => {
            if statement.if_not_exists && cassie.catalog.exists(&statement.table).await {
                return Ok(QueryResult {
                    columns: Vec::new(),
                    rows: Vec::new(),
                    command: "CREATE TABLE".to_string(),
                });
            }

            let schema = Schema {
                fields: statement
                    .fields
                    .iter()
                    .map(|field| FieldSchema {
                        name: field.name.clone(),
                        data_type: field.data_type.clone(),
                        nullable: true,
                    })
                    .collect(),
            };

            cassie
                .midge
                .create_collection(&statement.table, schema.clone())
                .await
                .map_err(|error| QueryError::General(error.to_string()))?;

            let constraints = statement
                .fields
                .iter()
                .flat_map(|field| field.constraints.iter().cloned())
                .collect::<Vec<_>>();

            cassie
                .midge
                .save_constraints(&statement.table, constraints.as_slice())
                .await
                .map_err(|error| QueryError::General(error.to_string()))?;
            cassie
                .catalog
                .register_collection_with_constraints(
                    &statement.table,
                    schema
                        .fields
                        .into_iter()
                        .map(|field| (field.name, field.data_type))
                        .collect(),
                    constraints,
                )
                .await;
            invalidate_plan_cache = true;

            Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "CREATE TABLE".to_string(),
            })
        }
        LogicalCommand::DropTable(statement) => {
            if statement.if_exists && !cassie.catalog.exists(&statement.table).await {
                return Ok(QueryResult {
                    columns: Vec::new(),
                    rows: Vec::new(),
                    command: "DROP TABLE".to_string(),
                });
            }

            cassie
                .midge
                .drop_collection(&statement.table)
                .await
                .map_err(|error| QueryError::General(error.to_string()))?;
            cassie.catalog.unregister_collection(&statement.table).await;
            invalidate_plan_cache = true;

            Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "DROP TABLE".to_string(),
            })
        }
        LogicalCommand::AlterTable(statement) => {
            match &statement.operation {
                crate::sql::ast::AlterTableOperation::AddColumn { field, data_type } => {
                    let field = FieldSchema {
                        name: field.clone(),
                        data_type: data_type.clone(),
                        nullable: true,
                    };
                    cassie
                        .midge
                        .alter_collection_add_column(&statement.table, field.clone())
                        .await
                        .map_err(|error| QueryError::General(error.to_string()))?;
                    cassie
                        .catalog
                        .add_collection_field(&statement.table, field.name, field.data_type.clone())
                        .await;
                    invalidate_plan_cache = true;
                }
                crate::sql::ast::AlterTableOperation::DropColumn { field } => {
                    cassie
                        .midge
                        .alter_collection_drop_column(&statement.table, field)
                        .await
                        .map_err(|error| QueryError::General(error.to_string()))?;
                    cassie
                        .catalog
                        .remove_collection_field(&statement.table, field)
                        .await;
                    invalidate_plan_cache = true;
                }
                crate::sql::ast::AlterTableOperation::RenameTo { table } => {
                    if cassie.catalog.exists(table).await {
                        return Err(QueryError::General(format!(
                            "collection '{table}' already exists"
                        )));
                    }

                    cassie
                        .midge
                        .rename_collection(&statement.table, table)
                        .await
                        .map_err(|error| QueryError::General(error.to_string()))?;
                    cassie
                        .catalog
                        .rename_collection(&statement.table, table)
                        .await;
                    invalidate_plan_cache = true;
                }
            }

            Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "ALTER TABLE".to_string(),
            })
        }
        LogicalCommand::CreateSchema(statement) => {
            if statement.if_not_exists && cassie.catalog.namespace_exists(&statement.schema).await {
                return Ok(QueryResult {
                    columns: Vec::new(),
                    rows: Vec::new(),
                    command: "CREATE SCHEMA".to_string(),
                });
            }

            cassie
                .midge
                .create_namespace(&statement.schema)
                .await
                .map_err(|error| QueryError::General(error.to_string()))?;
            cassie
                .catalog
                .register_namespace(&statement.schema, None)
                .await;
            invalidate_plan_cache = true;

            Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "CREATE SCHEMA".to_string(),
            })
        }
        LogicalCommand::CreateIndex(statement) => {
            if matches!(statement.kind, catalog::IndexKind::Vector) {
                let vector_index = vector_index_metadata(cassie, statement).await?;

                cassie
                    .midge
                    .put_vector_index(vector_index.clone())
                    .await
                    .map_err(|error| QueryError::General(error.to_string()))?;
                cassie.catalog.register_vector_index(vector_index).await;
            }

            let metadata = catalog::IndexMeta {
                collection: statement.table.clone(),
                name: statement.name.clone(),
                field: statement.field.clone(),
                kind: statement.kind.clone(),
                unique: statement.unique,
                options: statement.options.clone(),
            };

            cassie
                .midge
                .put_index(metadata.clone())
                .await
                .map_err(|error| QueryError::General(error.to_string()))?;
            cassie.catalog.register_index(metadata).await;
            invalidate_plan_cache = true;

            Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "CREATE INDEX".to_string(),
            })
        }
        LogicalCommand::DropIndex(statement) => {
            let index = cassie
                .catalog
                .get_index(&statement.table, &statement.name)
                .await;

            if statement.if_exists && index.is_none() {
                return Ok(QueryResult {
                    columns: Vec::new(),
                    rows: Vec::new(),
                    command: "DROP INDEX".to_string(),
                });
            }

            if let Some(index) = index {
                if matches!(index.kind, catalog::IndexKind::Vector) {
                    cassie
                        .midge
                        .delete_vector_index(&statement.table, &index.field)
                        .await
                        .map_err(|error| QueryError::General(error.to_string()))?;
                    cassie
                        .catalog
                        .unregister_vector_index(&statement.table, &index.field)
                        .await;
                }
            }

            cassie
                .midge
                .delete_index(&statement.table, &statement.name)
                .await
                .map_err(|error| QueryError::General(error.to_string()))?;
            cassie
                .catalog
                .unregister_index(&statement.table, &statement.name)
                .await;
            invalidate_plan_cache = true;

            Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "DROP INDEX".to_string(),
            })
        }
        LogicalCommand::CreateFunction(statement) => {
            if statement.if_not_exists
                && cassie.catalog.get_function(&statement.name).await.is_some()
            {
                return Ok(QueryResult {
                    columns: Vec::new(),
                    rows: Vec::new(),
                    command: "CREATE FUNCTION".to_string(),
                });
            }

            let metadata = FunctionMeta {
                name: statement.name.clone(),
                args: statement
                    .args
                    .iter()
                    .map(|arg| catalog::FunctionArgMeta {
                        name: arg.name.clone(),
                        data_type: arg.data_type.clone(),
                    })
                    .collect(),
                return_type: statement.return_type.clone(),
                volatility: Volatility::from(statement.volatility.clone()),
                body: statement.body.clone(),
            };

            cassie
                .midge
                .put_function(metadata.clone())
                .await
                .map_err(|error| QueryError::General(error.to_string()))?;
            cassie.catalog.register_function(metadata).await;
            invalidate_plan_cache = true;

            Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "CREATE FUNCTION".to_string(),
            })
        }
        LogicalCommand::DropFunction(statement) => {
            if statement.if_exists && cassie.catalog.get_function(&statement.name).await.is_none() {
                return Ok(QueryResult {
                    columns: Vec::new(),
                    rows: Vec::new(),
                    command: "DROP FUNCTION".to_string(),
                });
            }

            cassie
                .midge
                .delete_function(&statement.name)
                .await
                .map_err(|error| QueryError::General(error.to_string()))?;
            cassie.catalog.unregister_function(&statement.name).await;
            invalidate_plan_cache = true;

            Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "DROP FUNCTION".to_string(),
            })
        }
        LogicalCommand::CreateProcedure(statement) => {
            if statement.if_not_exists
                && cassie
                    .catalog
                    .get_procedure(&statement.name)
                    .await
                    .is_some()
            {
                return Ok(QueryResult {
                    columns: Vec::new(),
                    rows: Vec::new(),
                    command: "CREATE PROCEDURE".to_string(),
                });
            }

            let metadata = ProcedureMeta {
                name: statement.name.clone(),
                args: statement
                    .args
                    .iter()
                    .map(|arg| catalog::FunctionArgMeta {
                        name: arg.name.clone(),
                        data_type: arg.data_type.clone(),
                    })
                    .collect(),
                body: statement.body.clone(),
            };

            cassie
                .midge
                .put_procedure(metadata.clone())
                .await
                .map_err(|error| QueryError::General(error.to_string()))?;
            cassie.catalog.register_procedure(metadata).await;
            invalidate_plan_cache = true;

            Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "CREATE PROCEDURE".to_string(),
            })
        }
        LogicalCommand::DropProcedure(statement) => {
            if statement.if_exists
                && cassie
                    .catalog
                    .get_procedure(&statement.name)
                    .await
                    .is_none()
            {
                return Ok(QueryResult {
                    columns: Vec::new(),
                    rows: Vec::new(),
                    command: "DROP PROCEDURE".to_string(),
                });
            }

            cassie
                .midge
                .delete_procedure(&statement.name)
                .await
                .map_err(|error| QueryError::General(error.to_string()))?;
            cassie.catalog.unregister_procedure(&statement.name).await;
            invalidate_plan_cache = true;

            Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "DROP PROCEDURE".to_string(),
            })
        }
        LogicalCommand::CallProcedure(statement) => {
            if cassie
                .catalog
                .get_procedure(&statement.name)
                .await
                .is_none()
            {
                return Err(QueryError::General(format!(
                    "procedure '{}' does not exist",
                    statement.name
                )));
            }

            Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "CALL".to_string(),
            })
        }
    };

    if invalidate_plan_cache {
        cassie.invalidate_plan_cache();
    }

    result
}

async fn execute_insert(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    statement: &crate::sql::ast::InsertStatement,
    params: &[Value],
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let schema = cassie
        .catalog
        .get_schema(&statement.table)
        .await
        .ok_or_else(|| {
            QueryError::General(format!("collection '{}' not found", statement.table))
        })?;

    let source_rows =
        insert_source_rows(cassie, session, statement, params, user_functions, controls).await?;
    let source_width = source_rows
        .first()
        .map(Vec::len)
        .unwrap_or_else(|| insert_source_width(statement, &schema));
    let target_fields = insert_target_fields(statement, &schema, source_width)?;
    for row in &source_rows {
        if row.len() != target_fields.len() {
            return Err(QueryError::General(format!(
                "INSERT column/value counts mismatch: {} columns, {} values",
                target_fields.len(),
                row.len()
            )));
        }
    }

    let inserted_count = source_rows.len();
    let mut returning_rows = Vec::new();
    for source_row in source_rows {
        let payload = payload_from_insert_row(&target_fields, &source_row);
        let row_id = cassie
            .write_document_for_session(
                session,
                &statement.table,
                None,
                serde_json::Value::Object(payload),
                true,
                None,
            )
            .await
            .map_err(|error| QueryError::General(error.to_string()))?;

        if !statement.returning.is_empty() {
            let document = cassie
                .get_document_for_session(session, &statement.table, &row_id)
                .await
                .map_err(|error| QueryError::General(error.to_string()))?
                .ok_or_else(|| {
                    QueryError::General(format!(
                        "inserted row '{row_id}' was not found in '{}'",
                        statement.table
                    ))
                })?;

            returning_rows.push(inserted_row_to_batch_row(
                &row_id,
                &schema,
                &document.payload,
            ));
        }
    }

    if statement.returning.is_empty() {
        return Ok(QueryResult {
            columns: Vec::new(),
            rows: Vec::new(),
            command: format!("INSERT 0 {inserted_count}"),
        });
    }

    let projected = projection::project_rows(
        returning_rows,
        &statement.returning,
        params,
        None,
        user_functions,
        session,
    )?;

    Ok(QueryResult {
        columns: aggregate::columns_from_projection(&statement.returning),
        rows: projected.into_iter().map(BatchRow::into_values).collect(),
        command: format!("INSERT 0 {inserted_count}"),
    })
}

async fn insert_source_rows(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    statement: &crate::sql::ast::InsertStatement,
    params: &[Value],
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<Vec<Vec<Value>>, QueryError> {
    match &statement.source {
        InsertSource::Values(values) => values
            .iter()
            .map(|expr| {
                insert_expr_to_json(expr, params)
                    .map_err(QueryError::General)
                    .map(|value| json_to_value(&value))
            })
            .collect::<Result<Vec<_>, _>>()
            .map(|row| vec![row]),
        InsertSource::Select(select) => {
            let logical = LogicalPlan {
                command: None,
                source: select.source.clone(),
                collection: match &select.source {
                    QuerySource::Collection(name) | QuerySource::Cte(name) => name.clone(),
                    QuerySource::Subquery { alias, .. } => alias.clone(),
                    QuerySource::SingleRow => "single_row".to_string(),
                    QuerySource::Join { .. } => "join".to_string(),
                },
                ctes: select.ctes.clone(),
                distinct: select.distinct,
                projection: select.projection.clone(),
                filter: select.filter.clone(),
                group_by: select.group_by.clone(),
                having: select.having.clone(),
                order: select.order.clone(),
                limit: select.limit,
                offset: select.offset,
                set: select.set.clone(),
            };
            let mut cte_context = CteContext::new();
            let rows = execute_plan(
                cassie,
                session,
                &logical,
                &mut cte_context,
                user_functions,
                params,
                controls,
            )
            .await?;
            Ok(rows
                .into_iter()
                .map(|row| {
                    row.into_entries()
                        .into_iter()
                        .map(|(_, value)| value)
                        .collect()
                })
                .collect())
        }
    }
}

fn insert_source_width(
    statement: &crate::sql::ast::InsertStatement,
    schema: &CollectionSchema,
) -> usize {
    match &statement.source {
        InsertSource::Values(values) => values.len(),
        InsertSource::Select(select) => {
            if matches!(
                select.projection.as_slice(),
                [crate::sql::ast::SelectItem::Wildcard]
            ) {
                schema.fields.len()
            } else {
                select.projection.len()
            }
        }
    }
}

fn payload_from_insert_row(
    target_fields: &[FieldMeta],
    source_row: &[Value],
) -> serde_json::Map<String, serde_json::Value> {
    let mut payload = serde_json::Map::with_capacity(target_fields.len());
    for (field, value) in target_fields.iter().zip(source_row.iter()) {
        payload.insert(field.name.clone(), value_to_json(value));
    }
    payload
}

fn insert_target_fields(
    statement: &crate::sql::ast::InsertStatement,
    schema: &CollectionSchema,
    value_count: usize,
) -> Result<Vec<FieldMeta>, QueryError> {
    if statement.columns.is_empty() {
        if schema.fields.len() != value_count {
            return Err(QueryError::General(format!(
                "INSERT column/value counts mismatch: {} columns, {} values",
                schema.fields.len(),
                value_count
            )));
        }

        return Ok(schema.fields.clone());
    }

    if statement.columns.len() != value_count {
        return Err(QueryError::General(format!(
            "INSERT column/value counts mismatch: {} columns, {} values",
            statement.columns.len(),
            value_count
        )));
    }

    statement
        .columns
        .iter()
        .map(|column| {
            schema
                .fields
                .iter()
                .find(|field| field.name.eq_ignore_ascii_case(column))
                .cloned()
                .ok_or_else(|| {
                    QueryError::General(format!(
                        "INSERT target column '{}' does not exist in '{}'",
                        column, statement.table
                    ))
                })
        })
        .collect()
}

fn insert_expr_to_json(expr: &Expr, params: &[Value]) -> Result<serde_json::Value, String> {
    match expr {
        Expr::StringLiteral(value) => Ok(serde_json::Value::String(value.clone())),
        Expr::NumberLiteral(value) => number_literal_to_json(*value),
        Expr::BoolLiteral(value) => Ok(serde_json::Value::Bool(*value)),
        Expr::Null => Ok(serde_json::Value::Null),
        Expr::Param(index) => params
            .get(*index)
            .map(value_to_json)
            .ok_or_else(|| format!("missing bind parameter ${}", index + 1)),
        Expr::Column(_)
        | Expr::Function(_)
        | Expr::IsNull { .. }
        | Expr::InList { .. }
        | Expr::Between { .. }
        | Expr::Cast { .. }
        | Expr::Exists(_)
        | Expr::Binary {
            left: _,
            op: _,
            right: _,
        } => Err("INSERT VALUES only supports literals and bind parameters".to_string()),
    }
}

fn number_literal_to_json(value: f64) -> Result<serde_json::Value, String> {
    if !value.is_finite() {
        return Err("INSERT VALUES requires finite numeric literals".to_string());
    }

    if value.fract() == 0.0 && value >= i64::MIN as f64 && value <= i64::MAX as f64 {
        return Ok(serde_json::Value::Number((value as i64).into()));
    }

    serde_json::Number::from_f64(value)
        .map(serde_json::Value::Number)
        .ok_or_else(|| "INSERT VALUES requires finite numeric literals".to_string())
}

fn value_to_json(value: &Value) -> serde_json::Value {
    match value {
        Value::Null => serde_json::Value::Null,
        Value::Bool(value) => serde_json::Value::Bool(*value),
        Value::Int64(value) => serde_json::Value::Number((*value).into()),
        Value::Float64(value) => serde_json::Number::from_f64(*value)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        Value::String(value) => serde_json::Value::String(value.clone()),
        Value::Vector(value) => serde_json::Value::Array(
            value
                .values
                .iter()
                .filter_map(|value| serde_json::Number::from_f64((*value).into()))
                .map(serde_json::Value::Number)
                .collect(),
        ),
        Value::Json(value) => value.clone(),
    }
}

fn inserted_row_to_batch_row(
    row_id: &str,
    schema: &CollectionSchema,
    payload: &serde_json::Value,
) -> BatchRow {
    let mut row = Vec::with_capacity(schema.fields.len() + 1);
    row.push(("_id".to_string(), Value::String(row_id.to_string())));

    for field in &schema.fields {
        let value = payload
            .get(&field.name)
            .map(json_to_value)
            .unwrap_or(Value::Null);
        row.push((field.name.clone(), value));
    }

    BatchRow::new(row)
}

fn json_to_value(value: &serde_json::Value) -> Value {
    if value.is_null() {
        return Value::Null;
    }
    if let Some(value) = value.as_str() {
        return Value::String(value.to_string());
    }
    if let Some(value) = value.as_bool() {
        return Value::Bool(value);
    }
    if let Some(value) = value.as_i64() {
        return Value::Int64(value);
    }
    if let Some(value) = value.as_u64().and_then(|value| i64::try_from(value).ok()) {
        return Value::Int64(value);
    }
    if let Some(value) = value.as_f64() {
        return Value::Float64(value);
    }
    Value::Json(value.clone())
}

async fn execute_update(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    statement: &crate::sql::ast::UpdateStatement,
    params: &[Value],
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    check_timeout(controls)?;
    let schema = cassie
        .catalog
        .get_schema(&statement.table)
        .await
        .ok_or_else(|| {
            QueryError::General(format!("collection '{}' not found", statement.table))
        })?;

    let batches = scan::scan(cassie, session, &statement.table).await?;
    ensure_temp_budget(controls, &batches)?;
    let rows = batch::flatten_batches(batches);
    let matched_rows = if let Some(filter_expr) = &statement.filter {
        filter::filter_rows(rows, filter_expr, params, None, user_functions, session)?
    } else {
        rows
    };

    let mut prepared_rows = Vec::with_capacity(matched_rows.len());
    for row in &matched_rows {
        let row_id = row_id_from_batch_row(row)?;
        let current = cassie
            .get_document_for_session(session, &statement.table, &row_id)
            .await
            .map_err(|error| QueryError::General(error.to_string()))?
            .ok_or_else(|| {
                QueryError::General(format!(
                    "row '{row_id}' was not found in '{}'",
                    statement.table
                ))
            })?;
        let mut payload =
            current.payload.as_object().cloned().ok_or_else(|| {
                QueryError::General("stored row payload must be object".to_string())
            })?;

        for (field, expr) in &statement.assignments {
            let value = filter::evaluate_expr_value(
                row,
                expr,
                params,
                None,
                user_functions,
                session,
                None,
            )?;
            payload.insert(field.clone(), value_to_json(&value));
        }

        let payload = cassie
            .prepare_document_write_for_session(
                session,
                &statement.table,
                serde_json::Value::Object(payload),
                true,
                Some(&row_id),
            )
            .await
            .map_err(|error| QueryError::General(error.to_string()))?;
        prepared_rows.push((row_id, payload));
    }

    let mut returning_rows = Vec::new();
    for (row_id, payload) in prepared_rows {
        cassie
            .put_prepared_document_for_session(session, &statement.table, row_id.clone(), payload)
            .await
            .map_err(|error| QueryError::General(error.to_string()))?;

        if !statement.returning.is_empty() {
            let document = cassie
                .get_document_for_session(session, &statement.table, &row_id)
                .await
                .map_err(|error| QueryError::General(error.to_string()))?
                .ok_or_else(|| {
                    QueryError::General(format!(
                        "updated row '{row_id}' was not found in '{}'",
                        statement.table
                    ))
                })?;
            returning_rows.push(inserted_row_to_batch_row(
                &row_id,
                &schema,
                &document.payload,
            ));
        }
    }

    let updated_count = if statement.returning.is_empty() {
        matched_rows.len()
    } else {
        returning_rows.len()
    };
    if statement.returning.is_empty() {
        return Ok(QueryResult {
            columns: Vec::new(),
            rows: Vec::new(),
            command: format!("UPDATE {updated_count}"),
        });
    }

    let projected = projection::project_rows(
        returning_rows,
        &statement.returning,
        params,
        None,
        user_functions,
        session,
    )?;

    Ok(QueryResult {
        columns: aggregate::columns_from_projection(&statement.returning),
        rows: projected.into_iter().map(BatchRow::into_values).collect(),
        command: format!("UPDATE {updated_count}"),
    })
}

fn row_id_from_batch_row(row: &BatchRow) -> Result<String, QueryError> {
    match row.get("id") {
        Some(Value::String(value)) if !value.is_empty() => Ok(value.clone()),
        _ => Err(QueryError::General(
            "scanned row is missing internal row id".to_string(),
        )),
    }
}

async fn execute_delete(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    statement: &crate::sql::ast::DeleteStatement,
    params: &[Value],
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    check_timeout(controls)?;
    let schema = cassie
        .catalog
        .get_schema(&statement.table)
        .await
        .ok_or_else(|| {
            QueryError::General(format!("collection '{}' not found", statement.table))
        })?;

    let batches = scan::scan(cassie, session, &statement.table).await?;
    ensure_temp_budget(controls, &batches)?;
    let rows = batch::flatten_batches(batches);
    let matched_rows = if let Some(filter_expr) = &statement.filter {
        filter::filter_rows(rows, filter_expr, params, None, user_functions, session)?
    } else {
        rows
    };

    let mut delete_ids = Vec::with_capacity(matched_rows.len());
    let mut returning_rows = Vec::new();
    for row in &matched_rows {
        let row_id = row_id_from_batch_row(row)?;
        if !statement.returning.is_empty() {
            let current = cassie
                .get_document_for_session(session, &statement.table, &row_id)
                .await
                .map_err(|error| QueryError::General(error.to_string()))?
                .ok_or_else(|| {
                    QueryError::General(format!(
                        "row '{row_id}' was not found in '{}'",
                        statement.table
                    ))
                })?;
            returning_rows.push(inserted_row_to_batch_row(
                &row_id,
                &schema,
                &current.payload,
            ));
        }
        delete_ids.push(row_id);
    }

    for row_id in &delete_ids {
        cassie
            .delete_document_for_session(session, &statement.table, row_id)
            .await
            .map_err(|error| QueryError::General(error.to_string()))?;
    }

    let deleted_count = delete_ids.len();
    if statement.returning.is_empty() {
        return Ok(QueryResult {
            columns: Vec::new(),
            rows: Vec::new(),
            command: format!("DELETE {deleted_count}"),
        });
    }

    let projected = projection::project_rows(
        returning_rows,
        &statement.returning,
        params,
        None,
        user_functions,
        session,
    )?;

    Ok(QueryResult {
        columns: aggregate::columns_from_projection(&statement.returning),
        rows: projected.into_iter().map(BatchRow::into_values).collect(),
        command: format!("DELETE {deleted_count}"),
    })
}

async fn vector_index_metadata(
    cassie: &Cassie,
    statement: &crate::sql::ast::CreateIndexStatement,
) -> Result<VectorIndexRecord, QueryError> {
    let schema = cassie
        .midge
        .collection_schema(&statement.table)
        .await
        .ok_or_else(|| {
            QueryError::General(format!(
                "collection '{}' not found while creating vector index",
                statement.table
            ))
        })?;

    let vector_field = schema
        .fields
        .iter()
        .find(|field| field.name == statement.field)
        .ok_or_else(|| {
            QueryError::General(format!(
                "index field '{}' does not exist in collection '{}'",
                statement.field, statement.table
            ))
        })?;

    let dimensions = match vector_field.data_type {
        DataType::Vector(dimensions) => dimensions,
        _ => {
            return Err(QueryError::General(format!(
                "field '{}' is not a vector field",
                vector_field.name
            )));
        }
    };

    let source_field = statement
        .options
        .get("source_field")
        .ok_or_else(|| {
            QueryError::General("CREATE INDEX USING vector requires source_field".to_string())
        })?
        .to_string();

    let source_metadata = schema
        .fields
        .iter()
        .find(|field| field.name == source_field)
        .ok_or_else(|| {
            QueryError::General(format!(
                "source field '{}' does not exist in collection '{}'",
                source_field, statement.table
            ))
        })?;

    if !matches!(source_metadata.data_type, DataType::Text | DataType::Json) {
        return Err(QueryError::General(format!(
            "source field '{}' must be text/json for vector index",
            source_field
        )));
    }

    let metadata = VectorIndexMetadata {
        provider: cassie.embedding_provider.provider_name().to_string(),
        model: cassie.embedding_provider.model_name().to_string(),
        dimensions,
        metric: statement
            .options
            .get("metric")
            .and_then(|metric| metric.parse::<DistanceMetric>().ok())
            .unwrap_or(DistanceMetric::Cosine),
    };

    Ok(VectorIndexRecord {
        collection: statement.table.clone(),
        field: statement.field.clone(),
        source_field,
        metadata,
    })
}

async fn execute_plan(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    plan: &LogicalPlan,
    cte_context: &mut CteContext,
    user_functions: &HashMap<String, FunctionMeta>,
    params: &[Value],
    controls: &QueryExecutionControls,
) -> Result<Vec<BatchRow>, QueryError> {
    check_timeout(controls)?;
    if plan.command.is_some() {
        return Err(QueryError::General(
            "cannot execute command plans in CTE context".into(),
        ));
    }

    for cte in &plan.ctes {
        let rows = execute_cte(
            cassie,
            session,
            cte,
            cte_context,
            user_functions,
            params,
            controls,
        )
        .await?;
        cte_context.insert(cte.name.to_ascii_lowercase(), rows);
    }

    execute_source_query(
        cassie,
        session,
        plan,
        cte_context,
        user_functions,
        params,
        controls,
    )
    .await
}

fn execute_cte<'a>(
    cassie: &'a Cassie,
    session: Option<&'a CassieSession>,
    cte: &'a CommonTableExpression,
    cte_context: &'a mut CteContext,
    user_functions: &'a HashMap<String, FunctionMeta>,
    params: &'a [Value],
    controls: &'a QueryExecutionControls,
) -> CteExecution<'a> {
    Box::pin(async move {
        check_timeout(controls)?;
        let cte_name = cte.name.to_ascii_lowercase();
        let previous = cte_context.remove(&cte_name);

        let output = match &cte.query {
            CteQuery::Simple(statement) => {
                let logical = build_logical_plan(statement.as_ref())?;
                execute_plan(
                    cassie,
                    session,
                    &logical,
                    cte_context,
                    user_functions,
                    params,
                    controls,
                )
                .await?
                .into_iter()
                .map(BatchRow::into_entries)
                .collect()
            }
            CteQuery::Recursive { base, recursive } => {
                let base_plan = build_logical_plan(base.as_ref())?;
                let mut rows = execute_plan(
                    cassie,
                    session,
                    &base_plan,
                    cte_context,
                    user_functions,
                    params,
                    controls,
                )
                .await?
                .into_iter()
                .map(BatchRow::into_entries)
                .collect::<Vec<_>>();

                cte_context.insert(cte_name.clone(), rows.clone());

                let mut seen: HashSet<String> = rows.iter().map(row_signature).collect();
                let mut stabilized = false;

                for _ in 0..controls.cte_recursion_depth {
                    check_timeout(controls)?;
                    let recursive_plan = build_logical_plan(recursive.as_ref())?;
                    let recursive_rows = execute_plan(
                        cassie,
                        session,
                        &recursive_plan,
                        cte_context,
                        user_functions,
                        params,
                        controls,
                    )
                    .await?
                    .into_iter()
                    .map(BatchRow::into_entries)
                    .collect::<Vec<_>>();

                    let mut new_rows = Vec::new();
                    for row in recursive_rows {
                        let signature = row_signature(&row);
                        if seen.insert(signature) {
                            rows.push(row.clone());
                            new_rows.push(row);
                        }
                    }

                    if new_rows.is_empty() {
                        stabilized = true;
                        break;
                    }

                    ensure_temp_budget_for_rows(controls, &rows)?;
                    cte_context.insert(cte_name.clone(), rows.clone());
                }

                if !stabilized {
                    return Err(QueryError::General(format!(
                        "recursive CTE '{}' did not stabilize within {} iterations",
                        cte.name, controls.cte_recursion_depth
                    )));
                }

                rows
            }
        };

        if let Some(previous_rows) = previous {
            cte_context.insert(cte_name, previous_rows);
        } else {
            cte_context.remove(&cte_name);
        }

        Ok(output)
    })
}

fn resolve_exists_expr<'a>(
    cassie: &'a Cassie,
    session: Option<&'a CassieSession>,
    expr: &'a Expr,
    cte_context: &'a CteContext,
    user_functions: &'a HashMap<String, FunctionMeta>,
    params: &'a [Value],
    controls: &'a QueryExecutionControls,
) -> ExprResolution<'a> {
    Box::pin(async move {
        match expr {
            Expr::Binary { left, op, right } => Ok(Expr::Binary {
                left: Box::new(
                    resolve_exists_expr(
                        cassie,
                        session,
                        left,
                        cte_context,
                        user_functions,
                        params,
                        controls,
                    )
                    .await?,
                ),
                op: op.clone(),
                right: Box::new(
                    resolve_exists_expr(
                        cassie,
                        session,
                        right,
                        cte_context,
                        user_functions,
                        params,
                        controls,
                    )
                    .await?,
                ),
            }),
            Expr::IsNull { expr, negated } => Ok(Expr::IsNull {
                expr: Box::new(
                    resolve_exists_expr(
                        cassie,
                        session,
                        expr,
                        cte_context,
                        user_functions,
                        params,
                        controls,
                    )
                    .await?,
                ),
                negated: *negated,
            }),
            Expr::InList {
                expr,
                values,
                negated,
            } => {
                let expr = resolve_exists_expr(
                    cassie,
                    session,
                    expr,
                    cte_context,
                    user_functions,
                    params,
                    controls,
                )
                .await?;
                let mut resolved_values = Vec::with_capacity(values.len());
                for value in values {
                    resolved_values.push(
                        resolve_exists_expr(
                            cassie,
                            session,
                            value,
                            cte_context,
                            user_functions,
                            params,
                            controls,
                        )
                        .await?,
                    );
                }
                Ok(Expr::InList {
                    expr: Box::new(expr),
                    values: resolved_values,
                    negated: *negated,
                })
            }
            Expr::Between {
                expr,
                low,
                high,
                negated,
            } => Ok(Expr::Between {
                expr: Box::new(
                    resolve_exists_expr(
                        cassie,
                        session,
                        expr,
                        cte_context,
                        user_functions,
                        params,
                        controls,
                    )
                    .await?,
                ),
                low: Box::new(
                    resolve_exists_expr(
                        cassie,
                        session,
                        low,
                        cte_context,
                        user_functions,
                        params,
                        controls,
                    )
                    .await?,
                ),
                high: Box::new(
                    resolve_exists_expr(
                        cassie,
                        session,
                        high,
                        cte_context,
                        user_functions,
                        params,
                        controls,
                    )
                    .await?,
                ),
                negated: *negated,
            }),
            Expr::Cast { expr, data_type } => Ok(Expr::Cast {
                expr: Box::new(
                    resolve_exists_expr(
                        cassie,
                        session,
                        expr,
                        cte_context,
                        user_functions,
                        params,
                        controls,
                    )
                    .await?,
                ),
                data_type: data_type.clone(),
            }),
            Expr::Exists(statement) => {
                let logical = build_logical_plan(statement.as_ref())?;
                let mut subquery_context = cte_context.clone();
                let rows = execute_plan(
                    cassie,
                    session,
                    &logical,
                    &mut subquery_context,
                    user_functions,
                    params,
                    controls,
                )
                .await?;
                Ok(Expr::BoolLiteral(!rows.is_empty()))
            }
            Expr::Column(_)
            | Expr::Param(_)
            | Expr::StringLiteral(_)
            | Expr::NumberLiteral(_)
            | Expr::BoolLiteral(_)
            | Expr::Null
            | Expr::Function(_) => Ok(expr.clone()),
        }
    })
}

fn build_logical_plan(
    statement: &crate::sql::ast::ParsedStatement,
) -> Result<LogicalPlan, QueryError> {
    let plan = crate::planner::logical::plan(&crate::sql::binder::BoundStatement {
        statement: statement.clone(),
    })
    .map_err(|error| QueryError::General(error.to_string()))?;

    if plan.command.is_some() {
        return Err(QueryError::General(
            "CTE statements cannot include command statements".into(),
        ));
    }

    Ok(plan)
}

fn execute_query_source<'a>(
    env: &'a SourceExecutionEnv<'a>,
    source: &'a QuerySource,
    cte_context: &'a mut CteContext,
    qualify: bool,
) -> SourceExecution<'a> {
    Box::pin(async move {
        match source {
            QuerySource::Collection(name) => {
                if let Some(rows) = virtual_views::rows(&env.cassie.catalog, name).await {
                    let mut batches = materialize_virtual_rows(rows);
                    if qualify {
                        batches = qualify_batches(batches, name);
                    }
                    ensure_temp_budget(env.controls, &batches)?;
                    return Ok((batches, Vec::new()));
                }

                let mut batches = scan::scan(env.cassie, env.session, name).await?;
                if qualify {
                    batches = qualify_batches(batches, name);
                }
                ensure_temp_budget(env.controls, &batches)?;
                Ok((batches, env.cassie.catalog.text_fields(name).await))
            }
            QuerySource::SingleRow => {
                let batches =
                    batch::chunk_rows(vec![BatchRow::new(Vec::new())], batch::DEFAULT_BATCH_SIZE);
                ensure_temp_budget(env.controls, &batches)?;
                Ok((batches, Vec::new()))
            }
            QuerySource::Cte(name) => {
                let key = name.to_ascii_lowercase();
                let rows = cte_context.get(&key).cloned().ok_or_else(|| {
                    QueryError::General(format!("relation '{name}' does not exist"))
                })?;
                let text_fields = deduce_text_fields(&rows);
                let mut batches = batch::chunk_rows(
                    rows.into_iter().map(BatchRow::new).collect::<Vec<_>>(),
                    batch::DEFAULT_BATCH_SIZE,
                );
                if qualify {
                    batches = qualify_batches(batches, name);
                }
                ensure_temp_budget(env.controls, &batches)?;
                Ok((batches, text_fields))
            }
            QuerySource::Subquery { alias, select } => {
                let logical = LogicalPlan {
                    command: None,
                    source: select.source.clone(),
                    collection: alias.clone(),
                    ctes: select.ctes.clone(),
                    distinct: select.distinct,
                    projection: select.projection.clone(),
                    filter: select.filter.clone(),
                    group_by: select.group_by.clone(),
                    having: select.having.clone(),
                    order: select.order.clone(),
                    limit: select.limit,
                    offset: select.offset,
                    set: select.set.clone(),
                };
                let mut subquery_context = cte_context.clone();
                let rows = execute_plan(
                    env.cassie,
                    env.session,
                    &logical,
                    &mut subquery_context,
                    env.user_functions,
                    env.params,
                    env.controls,
                )
                .await?;
                let text_fields = deduce_text_fields(
                    &rows
                        .iter()
                        .map(|row| row.entries().to_vec())
                        .collect::<Vec<_>>(),
                );
                let batches =
                    qualify_batches(batch::chunk_rows(rows, batch::DEFAULT_BATCH_SIZE), alias);
                ensure_temp_budget(env.controls, &batches)?;
                Ok((batches, text_fields))
            }
            QuerySource::Join {
                left,
                right,
                kind,
                on,
            } => {
                let (left_batches, _left_text) =
                    execute_query_source(env, left, cte_context, true).await?;
                let (right_batches, _right_text) =
                    execute_query_source(env, right, cte_context, true).await?;
                let left_rows = batch::flatten_batches(left_batches);
                let right_rows = batch::flatten_batches(right_batches);
                let right_columns = row_columns(&right_rows);
                let mut joined = Vec::new();

                for left_row in &left_rows {
                    let mut matched = false;
                    for right_row in &right_rows {
                        let combined = combine_rows(left_row, right_row);
                        if filter::eval_scalar(
                            &combined,
                            on,
                            env.params,
                            None,
                            env.user_functions,
                            None,
                            env.session,
                        )?
                        .as_bool()
                        {
                            matched = true;
                            joined.push(combined);
                        }
                    }

                    if !matched && matches!(kind, JoinKind::Left) {
                        joined.push(combine_row_with_nulls(left_row, &right_columns));
                    }
                }

                let text_fields = deduce_text_fields(
                    &joined
                        .iter()
                        .map(|row| row.entries().to_vec())
                        .collect::<Vec<_>>(),
                );
                let batches = batch::chunk_rows(joined, batch::DEFAULT_BATCH_SIZE);
                ensure_temp_budget(env.controls, &batches)?;
                Ok((batches, text_fields))
            }
        }
    })
}

fn qualify_batches(batches: Vec<Batch>, qualifier: &str) -> Vec<Batch> {
    batches
        .into_iter()
        .map(|batch| {
            batch
                .into_iter()
                .map(|row| qualify_row(row, qualifier))
                .collect()
        })
        .collect()
}

fn qualify_row(row: BatchRow, qualifier: &str) -> BatchRow {
    let qualifier = qualifier.to_ascii_lowercase();
    let mut values = Vec::new();
    for (name, value) in row.into_entries() {
        values.push((name.clone(), value.clone()));
        values.push((format!("{qualifier}.{name}"), value));
    }
    BatchRow::new(values)
}

fn combine_rows(left: &BatchRow, right: &BatchRow) -> BatchRow {
    let mut values = left.entries().to_vec();
    values.extend(right.entries().iter().cloned());
    BatchRow::new(values)
}

fn combine_row_with_nulls(left: &BatchRow, right_columns: &[String]) -> BatchRow {
    let mut values = left.entries().to_vec();
    values.extend(
        right_columns
            .iter()
            .map(|column| (column.clone(), Value::Null)),
    );
    BatchRow::new(values)
}

fn row_columns(rows: &[BatchRow]) -> Vec<String> {
    let mut columns = Vec::new();
    for row in rows {
        for (column, _) in row.entries() {
            if !columns.contains(column) {
                columns.push(column.clone());
            }
        }
    }
    columns
}

fn materialize_virtual_rows(rows: Vec<virtual_views::VirtualRow>) -> Vec<Batch> {
    batch::chunk_rows(
        rows.into_iter().map(BatchRow::new).collect::<Vec<_>>(),
        batch::DEFAULT_BATCH_SIZE,
    )
}

#[derive(Clone)]
struct AggregateSpec {
    function: FunctionCall,
    output_names: Vec<String>,
}

fn aggregate_query_batches(
    batches: Vec<Batch>,
    plan: &LogicalPlan,
    params: &[Value],
    search_context: Option<&filter::SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
) -> Result<Vec<Batch>, QueryError> {
    let rows = batch::flatten_batches(batches);
    let specs = aggregate_specs(plan);
    let mut groups = BTreeMap::<String, (Vec<(String, Value)>, Vec<BatchRow>)>::new();

    for row in rows {
        let group_values = plan
            .group_by
            .iter()
            .map(|expr| {
                let name = group_expr_name(expr);
                let value = filter::evaluate_expr_value(
                    &row,
                    expr,
                    params,
                    search_context,
                    user_functions,
                    session,
                    None,
                )?;
                Ok((name, value))
            })
            .collect::<Result<Vec<_>, QueryError>>()?;
        let signature = if group_values.is_empty() {
            "__all__".to_string()
        } else {
            group_values
                .iter()
                .map(|(_, value)| value_sort_key(value))
                .collect::<Vec<_>>()
                .join("|")
        };
        groups
            .entry(signature)
            .or_insert_with(|| (group_values, Vec::new()))
            .1
            .push(row);
    }

    if groups.is_empty() && plan.group_by.is_empty() {
        groups.insert("__all__".to_string(), (Vec::new(), Vec::new()));
    }

    let mut out = Vec::with_capacity(groups.len());
    for (_signature, (group_values, group_rows)) in groups {
        let mut values = group_values;
        for spec in &specs {
            let value = evaluate_aggregate(
                &spec.function,
                &group_rows,
                params,
                search_context,
                user_functions,
                session,
            )?;
            for name in &spec.output_names {
                values.push((name.clone(), value.clone()));
            }
        }
        out.push(BatchRow::new(values));
    }

    Ok(batch::chunk_rows(out, batch::DEFAULT_BATCH_SIZE))
}

fn aggregate_specs(plan: &LogicalPlan) -> Vec<AggregateSpec> {
    let mut specs = Vec::<AggregateSpec>::new();
    for item in &plan.projection {
        if let SelectItem::Function { function, alias } = item {
            register_aggregate_spec(&mut specs, function, alias.clone());
        }
    }
    if let Some(having) = &plan.having {
        collect_aggregate_specs_from_expr(having, &mut specs);
    }
    for order in &plan.order {
        collect_aggregate_specs_from_expr(&order.expr, &mut specs);
    }
    specs
}

fn register_aggregate_spec(
    specs: &mut Vec<AggregateSpec>,
    function: &FunctionCall,
    alias: Option<String>,
) {
    if !crate::sql::functions::is_aggregate_function(&function.name) {
        return;
    }
    let signature = aggregate_signature(function);
    let output_name = alias.unwrap_or_else(|| function.name.clone());
    if let Some(existing) = specs
        .iter_mut()
        .find(|spec| aggregate_signature(&spec.function) == signature)
    {
        if !existing.output_names.contains(&output_name) {
            existing.output_names.push(output_name);
        }
        return;
    }
    let mut output_names = vec![function.name.clone()];
    if !output_names.contains(&output_name) {
        output_names.push(output_name);
    }
    specs.push(AggregateSpec {
        function: function.clone(),
        output_names,
    });
}

fn collect_aggregate_specs_from_expr(expr: &Expr, specs: &mut Vec<AggregateSpec>) {
    match expr {
        Expr::Function(function) => register_aggregate_spec(specs, function, None),
        Expr::Binary { left, right, .. } => {
            collect_aggregate_specs_from_expr(left, specs);
            collect_aggregate_specs_from_expr(right, specs);
        }
        Expr::IsNull { expr, .. } | Expr::Cast { expr, .. } => {
            collect_aggregate_specs_from_expr(expr, specs);
        }
        Expr::InList { expr, values, .. } => {
            collect_aggregate_specs_from_expr(expr, specs);
            for value in values {
                collect_aggregate_specs_from_expr(value, specs);
            }
        }
        Expr::Between {
            expr, low, high, ..
        } => {
            collect_aggregate_specs_from_expr(expr, specs);
            collect_aggregate_specs_from_expr(low, specs);
            collect_aggregate_specs_from_expr(high, specs);
        }
        Expr::Exists(_)
        | Expr::Column(_)
        | Expr::Param(_)
        | Expr::Null
        | Expr::BoolLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::StringLiteral(_) => {}
    }
}

fn evaluate_aggregate(
    function: &FunctionCall,
    rows: &[BatchRow],
    params: &[Value],
    search_context: Option<&filter::SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
) -> Result<Value, QueryError> {
    let name = function.name.to_ascii_lowercase();
    match name.as_str() {
        "count" => Ok(Value::Int64(count_aggregate(
            function,
            rows,
            params,
            search_context,
            user_functions,
            session,
        )?)),
        "sum" => sum_aggregate(
            function,
            rows,
            params,
            search_context,
            user_functions,
            session,
        ),
        "avg" => avg_aggregate(
            function,
            rows,
            params,
            search_context,
            user_functions,
            session,
        ),
        "min" => minmax_aggregate(
            function,
            rows,
            params,
            search_context,
            user_functions,
            false,
            session,
        ),
        "max" => minmax_aggregate(
            function,
            rows,
            params,
            search_context,
            user_functions,
            true,
            session,
        ),
        _ => Ok(Value::Null),
    }
}

fn count_aggregate(
    function: &FunctionCall,
    rows: &[BatchRow],
    params: &[Value],
    search_context: Option<&filter::SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
) -> Result<i64, QueryError> {
    if matches!(function.args.as_slice(), [Expr::Column(name)] if name == "*") {
        return Ok(rows.len() as i64);
    }
    let mut count = 0i64;
    for row in rows {
        let value = filter::evaluate_expr_value(
            row,
            &function.args[0],
            params,
            search_context,
            user_functions,
            session,
            None,
        )?;
        if !matches!(value, Value::Null) {
            count += 1;
        }
    }
    Ok(count)
}

fn sum_aggregate(
    function: &FunctionCall,
    rows: &[BatchRow],
    params: &[Value],
    search_context: Option<&filter::SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
) -> Result<Value, QueryError> {
    let mut sum = 0.0;
    let mut all_int = true;
    let mut seen = false;
    for row in rows {
        match filter::evaluate_expr_value(
            row,
            &function.args[0],
            params,
            search_context,
            user_functions,
            session,
            None,
        )? {
            Value::Int64(value) => {
                sum += value as f64;
                seen = true;
            }
            Value::Float64(value) => {
                sum += value;
                all_int = false;
                seen = true;
            }
            Value::Null => {}
            _ => all_int = false,
        }
    }
    if !seen {
        return Ok(Value::Null);
    }
    if all_int {
        Ok(Value::Int64(sum as i64))
    } else {
        Ok(Value::Float64(sum))
    }
}

fn avg_aggregate(
    function: &FunctionCall,
    rows: &[BatchRow],
    params: &[Value],
    search_context: Option<&filter::SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
) -> Result<Value, QueryError> {
    let mut sum = 0.0;
    let mut count = 0.0;
    for row in rows {
        match filter::evaluate_expr_value(
            row,
            &function.args[0],
            params,
            search_context,
            user_functions,
            session,
            None,
        )? {
            Value::Int64(value) => {
                sum += value as f64;
                count += 1.0;
            }
            Value::Float64(value) => {
                sum += value;
                count += 1.0;
            }
            _ => {}
        }
    }
    if count == 0.0 {
        Ok(Value::Null)
    } else {
        Ok(Value::Float64(sum / count))
    }
}

fn minmax_aggregate(
    function: &FunctionCall,
    rows: &[BatchRow],
    params: &[Value],
    search_context: Option<&filter::SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    max: bool,
    session: Option<&CassieSession>,
) -> Result<Value, QueryError> {
    let mut selected: Option<Value> = None;
    for row in rows {
        let value = filter::evaluate_expr_value(
            row,
            &function.args[0],
            params,
            search_context,
            user_functions,
            session,
            None,
        )?;
        if matches!(value, Value::Null) {
            continue;
        }
        let replace = selected
            .as_ref()
            .map(|current| {
                let current_key = value_sort_key(current);
                let value_key = value_sort_key(&value);
                if max {
                    value_key > current_key
                } else {
                    value_key < current_key
                }
            })
            .unwrap_or(true);
        if replace {
            selected = Some(value);
        }
    }
    Ok(selected.unwrap_or(Value::Null))
}

fn rewrite_aggregate_expr(expr: &Expr) -> Expr {
    match expr {
        Expr::Function(function)
            if crate::sql::functions::is_aggregate_function(&function.name) =>
        {
            Expr::Column(function.name.clone())
        }
        Expr::Binary { left, op, right } => Expr::Binary {
            left: Box::new(rewrite_aggregate_expr(left)),
            op: op.clone(),
            right: Box::new(rewrite_aggregate_expr(right)),
        },
        Expr::IsNull { expr, negated } => Expr::IsNull {
            expr: Box::new(rewrite_aggregate_expr(expr)),
            negated: *negated,
        },
        Expr::InList {
            expr,
            values,
            negated,
        } => Expr::InList {
            expr: Box::new(rewrite_aggregate_expr(expr)),
            values: values.iter().map(rewrite_aggregate_expr).collect(),
            negated: *negated,
        },
        Expr::Between {
            expr,
            low,
            high,
            negated,
        } => Expr::Between {
            expr: Box::new(rewrite_aggregate_expr(expr)),
            low: Box::new(rewrite_aggregate_expr(low)),
            high: Box::new(rewrite_aggregate_expr(high)),
            negated: *negated,
        },
        Expr::Cast { expr, data_type } => Expr::Cast {
            expr: Box::new(rewrite_aggregate_expr(expr)),
            data_type: data_type.clone(),
        },
        Expr::Exists(_)
        | Expr::Function(_)
        | Expr::Column(_)
        | Expr::Param(_)
        | Expr::Null
        | Expr::BoolLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::StringLiteral(_) => expr.clone(),
    }
}

fn distinct_batches(batches: Vec<Batch>) -> Vec<Batch> {
    let mut rows = BTreeMap::<String, BatchRow>::new();
    for row in batch::flatten_batches(batches) {
        rows.entry(row_signature(&row)).or_insert(row);
    }
    batch::chunk_rows(rows.into_values().collect(), batch::DEFAULT_BATCH_SIZE)
}

fn apply_set_operation(
    left: Vec<BatchRow>,
    right: Vec<BatchRow>,
    set: &SelectSet,
) -> Result<Vec<BatchRow>, QueryError> {
    validate_set_width(&left, &right)?;
    let mut rows = left;
    rows.extend(right);
    match set.operator {
        SetOperator::UnionAll => {
            rows.sort_by_key(row_signature);
            Ok(rows)
        }
        SetOperator::Union => {
            let mut unique = BTreeMap::<String, BatchRow>::new();
            for row in rows {
                unique.entry(row_signature(&row)).or_insert(row);
            }
            Ok(unique.into_values().collect())
        }
    }
}

fn validate_set_width(left: &[BatchRow], right: &[BatchRow]) -> Result<(), QueryError> {
    let left_width = left.first().map(|row| row.entries().len());
    let right_width = right.first().map(|row| row.entries().len());
    if let (Some(left_width), Some(right_width)) = (left_width, right_width) {
        if left_width != right_width {
            return Err(QueryError::General(format!(
                "set operation column count mismatch: {left_width} != {right_width}"
            )));
        }
    }
    Ok(())
}

fn logical_plan_from_select(select: &SelectStatement) -> LogicalPlan {
    LogicalPlan {
        command: None,
        source: select.source.clone(),
        collection: execution_source_name(&select.source),
        ctes: select.ctes.clone(),
        distinct: select.distinct,
        projection: select.projection.clone(),
        filter: select.filter.clone(),
        group_by: select.group_by.clone(),
        having: select.having.clone(),
        order: select.order.clone(),
        limit: select.limit,
        offset: select.offset,
        set: select.set.clone(),
    }
}

fn execution_source_name(source: &QuerySource) -> String {
    match source {
        QuerySource::Collection(name) | QuerySource::Cte(name) => name.clone(),
        QuerySource::Subquery { alias, .. } => alias.clone(),
        QuerySource::SingleRow => "single_row".to_string(),
        QuerySource::Join { .. } => "join".to_string(),
    }
}

fn plan_uses_aggregate(plan: &LogicalPlan) -> bool {
    !plan.group_by.is_empty()
        || plan.having.is_some()
        || plan.projection.iter().any(|item| match item {
            SelectItem::Function { function, .. } => {
                crate::sql::functions::is_aggregate_function(&function.name)
            }
            SelectItem::Wildcard | SelectItem::Column { .. } => false,
        })
}

fn group_expr_name(expr: &Expr) -> String {
    match expr {
        Expr::Column(name) => name.clone(),
        _ => expr_key(expr),
    }
}

fn aggregate_signature(function: &FunctionCall) -> String {
    format!(
        "{}({})",
        function.name.to_ascii_lowercase(),
        function
            .args
            .iter()
            .map(expr_key)
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn expr_key(expr: &Expr) -> String {
    match expr {
        Expr::Column(name) => name.clone(),
        Expr::Param(index) => format!("${}", index + 1),
        Expr::Null => "null".to_string(),
        Expr::BoolLiteral(value) => value.to_string(),
        Expr::NumberLiteral(value) => value.to_string(),
        Expr::StringLiteral(value) => format!("'{value}'"),
        Expr::Function(function) => aggregate_signature(function),
        Expr::Binary { left, op, right } => {
            format!("{}{:?}{}", expr_key(left), op, expr_key(right))
        }
        Expr::IsNull { expr, negated } => {
            format!(
                "{} is{} null",
                expr_key(expr),
                if *negated { " not" } else { "" }
            )
        }
        Expr::InList {
            expr,
            values,
            negated,
        } => format!(
            "{}{} in ({})",
            expr_key(expr),
            if *negated { " not" } else { "" },
            values.iter().map(expr_key).collect::<Vec<_>>().join(",")
        ),
        Expr::Between {
            expr,
            low,
            high,
            negated,
        } => format!(
            "{}{} between {} and {}",
            expr_key(expr),
            if *negated { " not" } else { "" },
            expr_key(low),
            expr_key(high)
        ),
        Expr::Cast { expr, data_type } => format!("{}::{data_type:?}", expr_key(expr)),
        Expr::Exists(_) => "exists".to_string(),
    }
}

fn value_sort_key(value: &Value) -> String {
    match value {
        Value::Null => "0:null".to_string(),
        Value::Bool(value) => format!("1:{value}"),
        Value::Int64(value) => format!("2:{value:020}"),
        Value::Float64(value) => format!("3:{value:020.12}"),
        Value::String(value) => format!("4:{value}"),
        Value::Vector(value) => format!("5:{:?}", value.values),
        Value::Json(value) => format!("6:{value}"),
    }
}

async fn execute_source_query(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    plan: &LogicalPlan,
    cte_context: &mut CteContext,
    user_functions: &HashMap<String, FunctionMeta>,
    params: &[Value],
    controls: &QueryExecutionControls,
) -> Result<Vec<BatchRow>, QueryError> {
    check_timeout(controls)?;
    let started_at = Instant::now();
    let env = SourceExecutionEnv {
        cassie,
        session,
        user_functions,
        params,
        controls,
    };
    let (mut batches, text_fields) =
        execute_query_source(&env, &plan.source, cte_context, false).await?;
    let candidate_rows = batches.iter().map(|batch| batch.len()).sum::<usize>();

    let fulltext_fields = fulltext_query_fields(plan);
    let uses_hybrid = plan_uses_function(plan, "hybrid_score");
    let uses_vector = plan_uses_vector_operator(plan);
    let search_context = if fulltext_fields.is_empty() {
        None
    } else {
        let (field_boost, field_k1, field_b) = if let QuerySource::Collection(name) = &plan.source {
            let fields = cassie.catalog.text_fields(name).await;
            let mut boost = HashMap::with_capacity(fields.len());
            for field in fields {
                if let Some(value) = cassie.catalog.get_field_boost(name, &field).await {
                    boost.insert(field, value as f64);
                }
            }

            let (index_boost, index_k1, index_b) =
                load_fulltext_index_options(cassie, name, &fulltext_fields).await?;
            for (field, value) in index_boost {
                boost.insert(field, value);
            }

            (boost, index_k1, index_b)
        } else {
            (HashMap::new(), HashMap::new(), HashMap::new())
        };

        Some(filter::SearchContext::from_rows(
            batches.iter().flat_map(|batch| batch.iter()),
            &text_fields,
            &field_boost,
            &field_k1,
            &field_b,
        ))
    };

    let resolved_filter = if let Some(filter_expr) = &plan.filter {
        Some(
            resolve_exists_expr(
                cassie,
                session,
                filter_expr,
                cte_context,
                user_functions,
                params,
                controls,
            )
            .await?,
        )
    } else {
        None
    };

    if let Some(filter_expr) = &resolved_filter {
        batches = filter::filter_batches(
            batches,
            filter_expr,
            params,
            search_context.as_ref(),
            user_functions,
            session,
        )?;
        ensure_temp_budget(controls, &batches)?;
    }

    if plan_uses_aggregate(plan) {
        batches = aggregate_query_batches(
            batches,
            plan,
            params,
            search_context.as_ref(),
            user_functions,
            session,
        )?;
        ensure_temp_budget(controls, &batches)?;
        if let Some(having) = &plan.having {
            let having = rewrite_aggregate_expr(having);
            batches = filter::filter_batches(
                batches,
                &having,
                params,
                search_context.as_ref(),
                user_functions,
                session,
            )?;
            ensure_temp_budget(controls, &batches)?;
        }
    }

    if !plan.order.is_empty() {
        batches = sort::sort_batches(
            batches,
            &plan.order,
            &plan.projection,
            params,
            search_context.as_ref(),
            user_functions,
            session,
        )?;
        ensure_temp_budget(controls, &batches)?;
    }

    batches = projection::project_batches(
        batches,
        &plan.projection,
        params,
        search_context.as_ref(),
        user_functions,
        session,
    )?;
    ensure_temp_budget(controls, &batches)?;

    if plan.distinct {
        batches = distinct_batches(batches);
        ensure_temp_budget(controls, &batches)?;
    }

    if let Some(offset) = plan.offset {
        let offset = offset.max(0) as usize;
        let limit = plan.limit.map(|value| value.max(0) as usize);
        batches = batch::slice_batches(batches, offset, limit);
    } else if let Some(limit) = plan.limit {
        let limit = limit.max(0) as usize;
        batches = batch::slice_batches(batches, 0, Some(limit));
    }

    let mut rows = batch::flatten_batches(batches);
    if let Some(set) = &plan.set {
        let right_plan = logical_plan_from_select(&set.right);
        let right_rows = Box::pin(execute_plan(
            cassie,
            session,
            &right_plan,
            cte_context,
            user_functions,
            params,
            controls,
        ))
        .await?;
        rows = apply_set_operation(rows, right_rows, set)?;
    }

    let elapsed = started_at.elapsed();
    if !fulltext_fields.is_empty() {
        cassie
            .runtime
            .record_search_execution(elapsed, candidate_rows, rows.len());
    }
    if uses_hybrid {
        cassie
            .runtime
            .record_hybrid_execution(elapsed, candidate_rows, rows.len());
    }
    if uses_vector {
        cassie
            .runtime
            .record_vector_execution(elapsed, candidate_rows, rows.len());
    }

    Ok(rows)
}

async fn load_fulltext_index_options(
    cassie: &Cassie,
    collection: &str,
    requested_fields: &HashSet<String>,
) -> Result<
    (
        HashMap<String, f64>,
        HashMap<String, f64>,
        HashMap<String, f64>,
    ),
    QueryError,
> {
    let mut field_boost = HashMap::new();
    let mut field_k1 = HashMap::new();
    let mut field_b = HashMap::new();
    let mut seen_fields = HashSet::new();

    for index in cassie.catalog.list_indexes(collection).await {
        if index.kind != catalog::IndexKind::FullText {
            continue;
        }

        let field = index.field.to_ascii_lowercase();
        if !requested_fields.contains(&field) {
            continue;
        }
        if !seen_fields.insert(field.clone()) {
            return Err(QueryError::General(format!(
                "fulltext indexes on field '{}' already exist on collection '{}'",
                index.field, collection
            )));
        }

        let boost = parse_index_float_option(
            &index,
            &index.field,
            "boost",
            index.options.get("boost").map(String::as_str),
            crate::search::bm25::DEFAULT_FULLTEXT_BOOST,
            0.0,
            None,
        )?;

        let k1 = parse_index_float_option(
            &index,
            &index.field,
            "k1",
            index.options.get("k1").map(String::as_str),
            crate::search::bm25::DEFAULT_BM25_K1,
            0.0,
            None,
        )?;

        let b = parse_index_float_option(
            &index,
            &index.field,
            "b",
            index.options.get("b").map(String::as_str),
            crate::search::bm25::DEFAULT_BM25_B,
            0.0,
            Some(1.0),
        )?;

        field_boost.insert(field.clone(), boost);
        field_k1.insert(field.clone(), k1);
        field_b.insert(field, b);
    }

    Ok((field_boost, field_k1, field_b))
}

fn fulltext_query_fields(plan: &LogicalPlan) -> HashSet<String> {
    let mut fields = HashSet::new();

    if let Some(filter) = &plan.filter {
        collect_fulltext_fields_from_expr(filter, &mut fields);
    }

    for order in &plan.order {
        collect_fulltext_fields_from_expr(&order.expr, &mut fields);
    }

    for item in &plan.projection {
        collect_fulltext_fields_from_select_item(item, &mut fields);
    }

    fields
}

fn plan_uses_function(plan: &LogicalPlan, function_name: &str) -> bool {
    if let Some(filter) = &plan.filter {
        if expr_uses_function(filter, function_name) {
            return true;
        }
    }

    if plan
        .order
        .iter()
        .any(|order| expr_uses_function(&order.expr, function_name))
    {
        return true;
    }

    if plan
        .projection
        .iter()
        .any(|item| select_item_uses_function(item, function_name))
    {
        return true;
    }

    plan.ctes
        .iter()
        .any(|cte| cte_uses_function(cte, function_name))
}

fn cte_uses_function(cte: &CommonTableExpression, function_name: &str) -> bool {
    match &cte.query {
        CteQuery::Simple(statement) => parsed_statement_uses_function(statement, function_name),
        CteQuery::Recursive { base, recursive } => {
            parsed_statement_uses_function(base, function_name)
                || parsed_statement_uses_function(recursive, function_name)
        }
    }
}

fn plan_uses_vector_operator(plan: &LogicalPlan) -> bool {
    if let Some(filter) = &plan.filter {
        if expr_uses_vector_operator(filter) {
            return true;
        }
    }

    if plan
        .order
        .iter()
        .any(|order| expr_uses_vector_operator(&order.expr))
    {
        return true;
    }

    if plan.projection.iter().any(select_item_uses_vector_operator) {
        return true;
    }

    plan.ctes.iter().any(cte_uses_vector_operator)
}

fn cte_uses_vector_operator(cte: &CommonTableExpression) -> bool {
    match &cte.query {
        CteQuery::Simple(statement) => parsed_statement_uses_vector_operator(statement),
        CteQuery::Recursive { base, recursive } => {
            parsed_statement_uses_vector_operator(base)
                || parsed_statement_uses_vector_operator(recursive)
        }
    }
}

fn function_uses_vector_operator(function: &crate::sql::ast::FunctionCall) -> bool {
    if function.name.eq_ignore_ascii_case("vector_distance")
        || function.name.eq_ignore_ascii_case("cosine_distance")
        || function.name.eq_ignore_ascii_case("dot_product")
        || function.name.eq_ignore_ascii_case("vector_score")
    {
        true
    } else {
        function.args.iter().any(expr_uses_vector_operator)
    }
}

fn select_item_uses_vector_operator(item: &crate::sql::ast::SelectItem) -> bool {
    match item {
        crate::sql::ast::SelectItem::Function { function, .. } => {
            function_uses_vector_operator(function)
        }
        _ => false,
    }
}

fn expr_uses_vector_operator(expr: &crate::sql::ast::Expr) -> bool {
    match expr {
        crate::sql::ast::Expr::Binary {
            left, right, op, ..
        } => {
            matches!(
                op,
                crate::sql::ast::BinaryOp::PgvectorCosine
                    | crate::sql::ast::BinaryOp::PgvectorL2
                    | crate::sql::ast::BinaryOp::PgvectorDot
            ) || expr_uses_vector_operator(left)
                || expr_uses_vector_operator(right)
        }
        crate::sql::ast::Expr::Function(function) => function_uses_vector_operator(function),
        crate::sql::ast::Expr::IsNull { expr, .. } => expr_uses_vector_operator(expr),
        crate::sql::ast::Expr::InList { expr, values, .. } => {
            expr_uses_vector_operator(expr) || values.iter().any(expr_uses_vector_operator)
        }
        crate::sql::ast::Expr::Between {
            expr, low, high, ..
        } => {
            expr_uses_vector_operator(expr)
                || expr_uses_vector_operator(low)
                || expr_uses_vector_operator(high)
        }
        crate::sql::ast::Expr::Cast { expr, .. } => expr_uses_vector_operator(expr),
        _ => false,
    }
}

fn parsed_statement_uses_vector_operator(statement: &crate::sql::ast::ParsedStatement) -> bool {
    match &statement.statement {
        crate::sql::ast::QueryStatement::Select(select) => select_uses_vector_operator(select),
        _ => false,
    }
}

fn select_uses_vector_operator(select: &crate::sql::ast::SelectStatement) -> bool {
    select
        .filter
        .as_ref()
        .is_some_and(expr_uses_vector_operator)
        || select
            .order
            .iter()
            .any(|order| expr_uses_vector_operator(&order.expr))
        || select.ctes.iter().any(cte_uses_vector_operator)
}

fn parsed_statement_uses_function(
    statement: &crate::sql::ast::ParsedStatement,
    function_name: &str,
) -> bool {
    match &statement.statement {
        crate::sql::ast::QueryStatement::Select(select) => {
            select_uses_function(select, function_name)
        }
        _ => false,
    }
}

fn select_uses_function(select: &crate::sql::ast::SelectStatement, function_name: &str) -> bool {
    select
        .projection
        .iter()
        .any(|item| select_item_uses_function(item, function_name))
        || select
            .filter
            .as_ref()
            .is_some_and(|expr| expr_uses_function(expr, function_name))
        || select
            .order
            .iter()
            .any(|order| expr_uses_function(&order.expr, function_name))
        || select
            .ctes
            .iter()
            .any(|cte| cte_uses_function(cte, function_name))
}

fn select_item_uses_function(item: &crate::sql::ast::SelectItem, function_name: &str) -> bool {
    match item {
        crate::sql::ast::SelectItem::Function { function, .. } => {
            function_uses_function(function, function_name)
        }
        _ => false,
    }
}

fn expr_uses_function(expr: &crate::sql::ast::Expr, function_name: &str) -> bool {
    match expr {
        crate::sql::ast::Expr::Binary { left, right, .. } => {
            expr_uses_function(left, function_name) || expr_uses_function(right, function_name)
        }
        crate::sql::ast::Expr::Function(function) => {
            function_uses_function(function, function_name)
        }
        crate::sql::ast::Expr::IsNull { expr, .. } => expr_uses_function(expr, function_name),
        crate::sql::ast::Expr::InList { expr, values, .. } => {
            expr_uses_function(expr, function_name)
                || values
                    .iter()
                    .any(|value| expr_uses_function(value, function_name))
        }
        crate::sql::ast::Expr::Between {
            expr, low, high, ..
        } => {
            expr_uses_function(expr, function_name)
                || expr_uses_function(low, function_name)
                || expr_uses_function(high, function_name)
        }
        crate::sql::ast::Expr::Cast { expr, .. } => expr_uses_function(expr, function_name),
        _ => false,
    }
}

fn function_uses_function(function: &crate::sql::ast::FunctionCall, function_name: &str) -> bool {
    function.name.eq_ignore_ascii_case(function_name)
        || function
            .args
            .iter()
            .any(|expr| expr_uses_function(expr, function_name))
}

fn collect_fulltext_fields_from_select_item(
    item: &crate::sql::ast::SelectItem,
    fields: &mut HashSet<String>,
) {
    if let crate::sql::ast::SelectItem::Function { function, .. } = item {
        collect_fulltext_fields_from_function(function, fields);
    }
}

fn collect_fulltext_fields_from_expr(expr: &crate::sql::ast::Expr, fields: &mut HashSet<String>) {
    match expr {
        crate::sql::ast::Expr::Binary { left, right, .. } => {
            collect_fulltext_fields_from_expr(left, fields);
            collect_fulltext_fields_from_expr(right, fields);
        }
        crate::sql::ast::Expr::Function(function) => {
            collect_fulltext_fields_from_function(function, fields);
        }
        crate::sql::ast::Expr::IsNull { expr, .. } => {
            collect_fulltext_fields_from_expr(expr, fields);
        }
        crate::sql::ast::Expr::InList { expr, values, .. } => {
            collect_fulltext_fields_from_expr(expr, fields);
            for value in values {
                collect_fulltext_fields_from_expr(value, fields);
            }
        }
        crate::sql::ast::Expr::Between {
            expr, low, high, ..
        } => {
            collect_fulltext_fields_from_expr(expr, fields);
            collect_fulltext_fields_from_expr(low, fields);
            collect_fulltext_fields_from_expr(high, fields);
        }
        crate::sql::ast::Expr::Cast { expr, .. } => {
            collect_fulltext_fields_from_expr(expr, fields);
        }
        _ => {}
    }
}

fn collect_fulltext_fields_from_function(
    function: &crate::sql::ast::FunctionCall,
    fields: &mut HashSet<String>,
) {
    let name = function.name.to_ascii_lowercase();
    if matches!(name.as_str(), "search" | "search_score") {
        if let Some(crate::sql::ast::Expr::Column(field)) = function.args.first() {
            fields.insert(field.to_ascii_lowercase());
        }
    }

    for arg in &function.args {
        collect_fulltext_fields_from_expr(arg, fields);
    }
}

fn parse_index_float_option(
    index: &catalog::IndexMeta,
    field: &str,
    key: &str,
    value: Option<&str>,
    default: f64,
    min: f64,
    max: Option<f64>,
) -> Result<f64, QueryError> {
    let value = value.unwrap_or("").trim();
    if value.is_empty() {
        return Ok(default);
    }

    let parsed = value.parse::<f64>().map_err(|_| {
        QueryError::General(format!(
            "fulltext index option '{key}' on '{field}' for collection '{}' must be numeric",
            index.collection
        ))
    })?;

    if !parsed.is_finite() {
        return Err(QueryError::General(format!(
            "fulltext index option '{key}' on '{field}' for collection '{}' must be finite",
            index.collection
        )));
    }

    let valid = if let Some(max) = max {
        parsed >= min && parsed <= max
    } else {
        parsed >= min
    };

    if !valid {
        if let Some(max) = max {
            return Err(QueryError::General(format!(
                "fulltext index option '{key}' on '{field}' for collection '{}' must be in [{min}, {max}]",
                index.collection
            )));
        }

        return Err(QueryError::General(format!(
            "fulltext index option '{key}' on '{field}' for collection '{}' must be at least {min}",
            index.collection
        )));
    }

    Ok(parsed)
}

fn check_timeout(controls: &QueryExecutionControls) -> Result<(), QueryError> {
    if controls.is_timed_out() {
        return Err(QueryError::General("query timeout exceeded".to_string()));
    }

    Ok(())
}

fn ensure_temp_budget(
    controls: &QueryExecutionControls,
    batches: &[batch::Batch],
) -> Result<(), QueryError> {
    let bytes = estimate_batch_bytes(batches);
    if bytes > controls.temp_spill_budget_bytes {
        return Err(QueryError::General(format!(
            "temporary storage budget exceeded: {bytes} > {}",
            controls.temp_spill_budget_bytes
        )));
    }

    Ok(())
}

fn ensure_temp_budget_for_rows(
    controls: &QueryExecutionControls,
    rows: &[Vec<(String, Value)>],
) -> Result<(), QueryError> {
    let bytes = rows
        .iter()
        .map(|row| {
            serde_json::to_vec(row)
                .map(|bytes| bytes.len())
                .unwrap_or_default()
        })
        .sum::<usize>();

    if bytes > controls.temp_spill_budget_bytes {
        return Err(QueryError::General(format!(
            "temporary storage budget exceeded: {bytes} > {}",
            controls.temp_spill_budget_bytes
        )));
    }

    Ok(())
}

fn estimate_batch_bytes(batches: &[batch::Batch]) -> usize {
    batches
        .iter()
        .flat_map(|batch| batch.iter())
        .map(|row| {
            serde_json::to_vec(row.entries())
                .map(|bytes| bytes.len())
                .unwrap_or_default()
        })
        .sum()
}

fn row_signature(row: &impl RowAccess) -> String {
    serde_json::to_string(row.entries()).unwrap_or_else(|_| String::new())
}

fn deduce_text_fields<R: RowAccess>(rows: &[R]) -> Vec<String> {
    let mut fields = HashSet::<String>::new();
    let mut ordered = Vec::new();

    for row in rows {
        for (name, value) in row.entries() {
            if !matches!(value, Value::String(_) | Value::Json(_)) {
                continue;
            }

            let name = name.to_ascii_lowercase();
            if fields.insert(name.clone()) {
                ordered.push(name);
            }
        }
    }

    ordered
}
