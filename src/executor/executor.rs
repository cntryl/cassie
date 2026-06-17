use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;

use crate::app::Cassie;
use crate::catalog;
use crate::catalog::{FunctionMeta, ProcedureMeta, Volatility};
use crate::executor::batch::{self, BatchRow, RowAccess};
use crate::executor::{aggregate, filter, projection, scan, sort};
use crate::planner::logical::{LogicalCommand, LogicalPlan};
use crate::planner::physical::PhysicalPlan;
use crate::sql::ast::{CommonTableExpression, CteQuery, QuerySource};
use crate::types::{FieldSchema, Schema, Value};

const MAX_RECURSIVE_CTE_DEPTH: usize = 64;

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

pub async fn run(
    cassie: &Cassie,
    plan: PhysicalPlan,
    params: Vec<Value>,
) -> Result<QueryResult, QueryError> {
    let user_functions = cassie
        .catalog
        .list_functions()
        .await
        .into_iter()
        .map(|metadata| (metadata.name.to_ascii_lowercase(), metadata))
        .collect::<HashMap<String, FunctionMeta>>();

    if let Some(command) = plan.logical.command.as_ref() {
        return execute_command(cassie, command).await;
    }

    let mut cte_context: CteContext = HashMap::new();
    let rows = execute_plan(
        cassie,
        &plan.logical,
        &mut cte_context,
        &user_functions,
        &params,
    )
    .await?;

    let columns = aggregate::columns_from_projection(&plan.logical.projection);
    let rows = rows.into_iter().map(BatchRow::into_values).collect();

    Ok(QueryResult {
        columns,
        rows,
        command: "SELECT".to_string(),
    })
}

async fn execute_command(
    cassie: &Cassie,
    command: &LogicalCommand,
) -> Result<QueryResult, QueryError> {
    match command {
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

            Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "CREATE SCHEMA".to_string(),
            })
        }
        LogicalCommand::CreateIndex(statement) => {
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

            Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "CREATE INDEX".to_string(),
            })
        }
        LogicalCommand::DropIndex(statement) => {
            if statement.if_exists {
                let existing = cassie
                    .catalog
                    .get_index(&statement.table, &statement.name)
                    .await
                    .is_some();
                if !existing {
                    return Ok(QueryResult {
                        columns: Vec::new(),
                        rows: Vec::new(),
                        command: "DROP INDEX".to_string(),
                    });
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
    }
}

async fn execute_plan(
    cassie: &Cassie,
    plan: &LogicalPlan,
    cte_context: &mut CteContext,
    user_functions: &HashMap<String, FunctionMeta>,
    params: &[Value],
) -> Result<Vec<BatchRow>, QueryError> {
    if plan.command.is_some() {
        return Err(QueryError::General(
            "cannot execute command plans in CTE context".into(),
        ));
    }

    for cte in &plan.ctes {
        let rows = execute_cte(cassie, cte, cte_context, user_functions, params).await?;
        cte_context.insert(cte.name.to_ascii_lowercase(), rows);
    }

    execute_source_query(cassie, plan, cte_context, user_functions, params).await
}

fn execute_cte<'a>(
    cassie: &'a Cassie,
    cte: &'a CommonTableExpression,
    cte_context: &'a mut CteContext,
    user_functions: &'a HashMap<String, FunctionMeta>,
    params: &'a [Value],
) -> CteExecution<'a> {
    Box::pin(async move {
        let cte_name = cte.name.to_ascii_lowercase();
        let previous = cte_context.remove(&cte_name);

        let output = match &cte.query {
            CteQuery::Simple(statement) => {
                let logical = build_logical_plan(statement.as_ref())?;
                execute_plan(cassie, &logical, cte_context, user_functions, params)
                    .await?
                    .into_iter()
                    .map(BatchRow::into_entries)
                    .collect()
            }
            CteQuery::Recursive { base, recursive } => {
                let base_plan = build_logical_plan(base.as_ref())?;
                let mut rows =
                    execute_plan(cassie, &base_plan, cte_context, user_functions, params)
                        .await?
                        .into_iter()
                        .map(BatchRow::into_entries)
                        .collect::<Vec<_>>();

                cte_context.insert(cte_name.clone(), rows.clone());

                let mut seen: HashSet<String> = rows.iter().map(row_signature).collect();
                let mut stabilized = false;

                for _ in 0..MAX_RECURSIVE_CTE_DEPTH {
                    let recursive_plan = build_logical_plan(recursive.as_ref())?;
                    let recursive_rows =
                        execute_plan(cassie, &recursive_plan, cte_context, user_functions, params)
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

                    cte_context.insert(cte_name.clone(), rows.clone());
                }

                if !stabilized {
                    return Err(QueryError::General(format!(
                        "recursive CTE '{}' did not stabilize within {} iterations",
                        cte.name, MAX_RECURSIVE_CTE_DEPTH
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

async fn execute_source_query(
    cassie: &Cassie,
    plan: &LogicalPlan,
    cte_context: &mut CteContext,
    user_functions: &HashMap<String, FunctionMeta>,
    params: &[Value],
) -> Result<Vec<BatchRow>, QueryError> {
    let (mut batches, text_fields) = match &plan.source {
        QuerySource::Collection(name) => {
            let batches = scan::scan(cassie, name).await?;
            (batches, cassie.catalog.text_fields(name).await)
        }
        QuerySource::Cte(name) => {
            let key = name.to_ascii_lowercase();
            let rows = cte_context
                .get(&key)
                .cloned()
                .ok_or_else(|| QueryError::General(format!("relation '{name}' does not exist")))?;
            let text_fields = deduce_text_fields(&rows);
            let batches = batch::chunk_rows(
                rows.into_iter().map(BatchRow::new).collect::<Vec<_>>(),
                batch::DEFAULT_BATCH_SIZE,
            );
            (batches, text_fields)
        }
    };

    let field_boost = if let QuerySource::Collection(name) = &plan.source {
        let fields = cassie.catalog.text_fields(name).await;
        let mut boost = HashMap::with_capacity(fields.len());
        for field in fields {
            if let Some(value) = cassie.catalog.get_field_boost(name, &field).await {
                boost.insert(field, value as f64);
            }
        }
        boost
    } else {
        HashMap::new()
    };

    let search_context = filter::SearchContext::from_rows(
        batches.iter().flat_map(|batch| batch.iter()),
        &text_fields,
        &field_boost,
    );

    if let Some(filter_expr) = &plan.filter {
        batches = filter::filter_batches(
            batches,
            filter_expr,
            params,
            Some(&search_context),
            user_functions,
        )?;
    }

    if !plan.order.is_empty() {
        batches = sort::sort_batches(
            batches,
            &plan.order,
            &plan.projection,
            params,
            Some(&search_context),
            user_functions,
        )?;
    }

    batches = projection::project_batches(
        batches,
        &plan.projection,
        params,
        Some(&search_context),
        user_functions,
    )?;

    if let Some(offset) = plan.offset {
        let offset = offset.max(0) as usize;
        let limit = plan.limit.map(|value| value.max(0) as usize);
        batches = batch::slice_batches(batches, offset, limit);
    } else if let Some(limit) = plan.limit {
        let limit = limit.max(0) as usize;
        batches = batch::slice_batches(batches, 0, Some(limit));
    }

    Ok(batch::flatten_batches(batches))
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
