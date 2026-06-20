use std::cmp::Ordering as CmpOrdering;
use std::collections::{BTreeMap, BinaryHeap, HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::app::{Cassie, CassieSession};
use crate::catalog;
use crate::catalog::virtual_views;
use crate::catalog::{CollectionSchema, FieldMeta, FunctionMeta, ProcedureMeta, Volatility};
use crate::embeddings::{DistanceMetric, VectorIndexMetadata, VectorIndexRecord};
use crate::executor::batch::{self, Batch, BatchRow, RowAccess};
use crate::executor::{aggregate, filter, projection, scan, sort};
use crate::midge::adapter::RowDecode;
use crate::planner::logical::{LogicalCommand, LogicalPlan};
use crate::planner::physical::PhysicalPlan;
use crate::query_cache;
use crate::runtime::{FulltextIndexOptions, FulltextIndexOptionsCacheKey, QueryExecutionControls};
use crate::sql::ast::{
    BinaryOp, CommonTableExpression, CteQuery, Expr, FunctionCall, InsertSource, JoinKind,
    QuerySource, QueryStatement, SelectItem, SelectSet, SelectStatement, SetOperator,
    SortDirection,
};
use crate::types::{DataType, FieldSchema, Schema, Value};

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct ColumnMeta {
    pub name: String,
    pub data_type: String,
    pub type_oid: i64,
    pub typlen: i16,
    pub atttypmod: i32,
    pub format_code: i16,
    pub nullable: bool,
}

impl ColumnMeta {
    pub fn text(name: impl Into<String>) -> Self {
        Self::from_data_type(name, DataType::Text)
    }

    pub fn from_data_type(name: impl Into<String>, data_type: DataType) -> Self {
        let data_type_name = data_type.type_name();
        Self {
            name: name.into(),
            data_type: data_type_name,
            type_oid: data_type.type_oid(),
            typlen: data_type.typlen(),
            atttypmod: data_type.atttypmod(),
            format_code: 0,
            nullable: true,
        }
    }
}

fn primary_key_indexes(
    table: &str,
    constraints: &[catalog::FieldConstraint],
) -> Vec<catalog::IndexMeta> {
    constraints
        .iter()
        .filter(|constraint| constraint.primary_key)
        .map(|constraint| catalog::IndexMeta {
            collection: table.to_string(),
            name: format!("{table}_pkey"),
            field: constraint.field.clone(),
            fields: vec![constraint.field.clone()],
            kind: catalog::IndexKind::Scalar,
            unique: true,
            options: BTreeMap::new(),
        })
        .collect()
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct QueryResult {
    pub columns: Vec<ColumnMeta>,
    pub rows: Vec<Vec<Value>>,
    pub command: String,
}

#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
pub struct ExecutionBreakdownMicros {
    pub scan_us: u64,
    pub row_decode_us: u64,
    pub filter_us: u64,
    pub projection_us: u64,
    pub sort_us: u64,
    pub result_build_us: u64,
    pub stats_us: u64,
}

#[derive(Debug, Clone)]
pub struct ExecutionBreakdownOutput {
    pub result: QueryResult,
    pub breakdown: ExecutionBreakdownMicros,
}

#[derive(Debug, Clone, Copy, Default)]
struct ExecutionBreakdownDurations {
    scan: Duration,
    row_decode: Duration,
    filter: Duration,
    projection: Duration,
    sort: Duration,
    result_build: Duration,
    stats: Duration,
}

impl ExecutionBreakdownDurations {
    fn into_micros(self) -> ExecutionBreakdownMicros {
        ExecutionBreakdownMicros {
            scan_us: duration_micros(self.scan),
            row_decode_us: duration_micros(self.row_decode),
            filter_us: duration_micros(self.filter),
            projection_us: duration_micros(self.projection),
            sort_us: duration_micros(self.sort),
            result_build_us: duration_micros(self.result_build),
            stats_us: duration_micros(self.stats),
        }
    }
}

fn duration_micros(duration: Duration) -> u64 {
    duration.as_micros().try_into().unwrap_or(u64::MAX)
}

#[derive(Debug)]
pub enum QueryError {
    General(String),
}

type CteRows = Vec<Vec<(String, Value)>>;
type CteContext = HashMap<String, CteRows>;
type CteExecution<'a> = Result<CteRows, QueryError>;
type ExprResolution<'a> = Result<Expr, QueryError>;
type SourceExecution<'a> = Result<(Vec<Batch>, Vec<String>), QueryError>;

struct SourceExecutionEnv<'a> {
    cassie: &'a Cassie,
    session: Option<&'a CassieSession>,
    user_functions: &'a HashMap<String, FunctionMeta>,
    params: &'a [Value],
    controls: &'a QueryExecutionControls,
}

pub fn run(
    cassie: &Cassie,
    plan: PhysicalPlan,
    params: Vec<Value>,
) -> Result<QueryResult, QueryError> {
    let controls = cassie.runtime.query_controls(std::time::Instant::now());
    run_with_controls(cassie, Arc::new(plan), params, &controls)
}

pub fn run_with_controls(
    cassie: &Cassie,
    plan: Arc<PhysicalPlan>,
    params: Vec<Value>,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    run_with_session_controls(cassie, None, plan, params, controls)
}

#[doc(hidden)]
pub fn run_with_execution_breakdown(
    cassie: &Cassie,
    plan: PhysicalPlan,
    params: Vec<Value>,
) -> Result<ExecutionBreakdownOutput, QueryError> {
    let controls = cassie.runtime.query_controls(std::time::Instant::now());
    run_with_execution_breakdown_controls(cassie, Arc::new(plan), params, &controls)
}

fn run_with_execution_breakdown_controls(
    cassie: &Cassie,
    plan: Arc<PhysicalPlan>,
    params: Vec<Value>,
    controls: &QueryExecutionControls,
) -> Result<ExecutionBreakdownOutput, QueryError> {
    let user_functions =
        if plan.logical.command.is_some() || plan_needs_user_functions(&plan.logical) {
            cassie
                .catalog
                .list_functions()
                
                .into_iter()
                .map(|metadata| (metadata.name.to_ascii_lowercase(), metadata))
                .collect::<HashMap<String, FunctionMeta>>()
        } else {
            HashMap::new()
        };

    if let Some(command) = plan.logical.command.as_ref() {
        let started = Instant::now();
        let result =
            execute_command(cassie, None, command, &params, &user_functions, controls)?;
        let breakdown = ExecutionBreakdownDurations {
            result_build: started.elapsed(),
            ..Default::default()
        };
        return Ok(ExecutionBreakdownOutput {
            result,
            breakdown: breakdown.into_micros(),
        });
    }

    let mut cte_context: CteContext = HashMap::new();
    let (rows, mut breakdown) = execute_plan_with_execution_breakdown(
        cassie,
        None,
        &plan.logical,
        &mut cte_context,
        &user_functions,
        &params,
        controls,
    )
    ?;

    let result_started = Instant::now();
    let collection_schema = cassie.catalog.get_schema(&plan.logical.collection);
    let columns = aggregate::columns_from_projection(
        &plan.logical.projection,
        collection_schema.as_ref(),
        &user_functions,
    );
    let rows: Vec<Vec<Value>> = rows.into_iter().map(BatchRow::into_values).collect();

    if rows.len() > controls.max_result_rows {
        return Err(QueryError::General(format!(
            "query result row limit exceeded: {} > {}",
            rows.len(),
            controls.max_result_rows
        )));
    }

    breakdown.result_build += result_started.elapsed();
    Ok(ExecutionBreakdownOutput {
        result: QueryResult {
            columns,
            rows,
            command: "SELECT".to_string(),
        },
        breakdown: breakdown.into_micros(),
    })
}

pub(crate) fn run_with_session_controls(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    plan: Arc<PhysicalPlan>,
    params: Vec<Value>,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let user_functions =
        if plan.logical.command.is_some() || plan_needs_user_functions(&plan.logical) {
            cassie
                .catalog
                .list_functions()
                
                .into_iter()
                .map(|metadata| (metadata.name.to_ascii_lowercase(), metadata))
                .collect::<HashMap<String, FunctionMeta>>()
        } else {
            HashMap::new()
        };

    if let Some(command) = plan.logical.command.as_ref() {
        return execute_command(cassie, session, command, &params, &user_functions, controls);
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
    ?;

    let collection_schema = cassie.catalog.get_schema(&plan.logical.collection);
    let columns = aggregate::columns_from_projection(
        &plan.logical.projection,
        collection_schema.as_ref(),
        &user_functions,
    );
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

fn execute_command(
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
                    columns: vec![ColumnMeta::text("search_path")],
                    rows: vec![vec![Value::String("public".to_string())]],
                    command: "SHOW".to_string(),
                }),
                "server_version" => Ok(QueryResult {
                    columns: vec![ColumnMeta::text("server_version")],
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
            execute_insert(cassie, session, statement, params, user_functions, controls)
        }
        LogicalCommand::Update(statement) => {
            execute_update(cassie, session, statement, params, user_functions, controls)
        }
        LogicalCommand::Delete(statement) => {
            execute_delete(cassie, session, statement, params, user_functions, controls)
        }
        LogicalCommand::CreateTable(statement) => {
            if statement.if_not_exists
                && (cassie.catalog.relation_exists(&statement.table)
                    || virtual_views::schema(&statement.table).is_some())
            {
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
                .map_err(|error| QueryError::General(error.to_string()))?;

            let constraints = statement
                .fields
                .iter()
                .flat_map(|field| field.constraints.iter().cloned())
                .collect::<Vec<_>>();

            cassie
                .midge
                .save_constraints(&statement.table, constraints.as_slice())
                .map_err(|error| QueryError::General(error.to_string()))?;
            let primary_key_indexes = primary_key_indexes(&statement.table, constraints.as_slice());
            for index in &primary_key_indexes {
                cassie
                    .midge
                    .put_index(index.clone())
                    .map_err(|error| QueryError::General(error.to_string()))?;
            }
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
                ;
            for index in primary_key_indexes {
                cassie.catalog.register_index(index);
            }
            invalidate_plan_cache = true;

            Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "CREATE TABLE".to_string(),
            })
        }
        LogicalCommand::CreateView(statement) => {
            if statement.if_not_exists
                && (cassie.catalog.relation_exists(&statement.name)
                    || virtual_views::schema(&statement.name).is_some())
            {
                return Ok(QueryResult {
                    columns: Vec::new(),
                    rows: Vec::new(),
                    command: "CREATE VIEW".to_string(),
                });
            }

            let parsed = crate::sql::parser::parse_statement(&statement.query)
                .map_err(|error| QueryError::General(error.0))?;
            let bound = crate::sql::binder::bind(parsed, &cassie.catalog)
                
                .map_err(|error| QueryError::General(error.to_string()))?;
            let QueryStatement::Select(select) = &bound.statement.statement else {
                return Err(QueryError::General(
                    "CREATE VIEW requires a SELECT query body".to_string(),
                ));
            };

            let schema = crate::sql::binder::infer_select_schema(select, &cassie.catalog)
                
                .map_err(|error| QueryError::General(error.to_string()))?;
            let metadata = crate::catalog::ViewMeta::new(
                statement.name.clone(),
                statement.query.clone(),
                schema,
            );

            cassie
                .midge
                .put_view(metadata.clone())
                .map_err(|error| QueryError::General(error.to_string()))?;
            cassie.catalog.register_view(metadata);
            invalidate_plan_cache = true;

            Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "CREATE VIEW".to_string(),
            })
        }
        LogicalCommand::DropView(statement) => {
            let view = cassie.catalog.get_view(&statement.name);
            if statement.if_exists && view.is_none() {
                return Ok(QueryResult {
                    columns: Vec::new(),
                    rows: Vec::new(),
                    command: "DROP VIEW".to_string(),
                });
            }

            let Some(_) = view else {
                return Err(QueryError::General(format!(
                    "view '{}' does not exist",
                    statement.name
                )));
            };

            cassie
                .midge
                .delete_view(&statement.name)
                .map_err(|error| QueryError::General(error.to_string()))?;
            cassie.catalog.unregister_view(&statement.name);
            invalidate_plan_cache = true;

            Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "DROP VIEW".to_string(),
            })
        }
        LogicalCommand::DropTable(statement) => {
            if statement.if_exists && !cassie.catalog.exists(&statement.table) {
                return Ok(QueryResult {
                    columns: Vec::new(),
                    rows: Vec::new(),
                    command: "DROP TABLE".to_string(),
                });
            }

            cassie
                .midge
                .drop_collection(&statement.table)
                .map_err(|error| QueryError::General(error.to_string()))?;
            cassie.catalog.unregister_collection(&statement.table);
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
                        .map_err(|error| QueryError::General(error.to_string()))?;
                    cassie
                        .catalog
                        .add_collection_field(&statement.table, field.name, field.data_type.clone())
                        ;
                    invalidate_plan_cache = true;
                }
                crate::sql::ast::AlterTableOperation::DropColumn { field } => {
                    cassie
                        .midge
                        .alter_collection_drop_column(&statement.table, field)
                        .map_err(|error| QueryError::General(error.to_string()))?;
                    cassie
                        .catalog
                        .remove_collection_field(&statement.table, field)
                        ;
                    invalidate_plan_cache = true;
                }
                crate::sql::ast::AlterTableOperation::RenameColumn { from, to } => {
                    cassie
                        .midge
                        .alter_collection_rename_column(&statement.table, from, to)
                        .map_err(|error| QueryError::General(error.to_string()))?;
                    cassie
                        .catalog
                        .rename_collection_field(&statement.table, from, to)
                        ;
                    invalidate_plan_cache = true;
                }
                crate::sql::ast::AlterTableOperation::RenameTo { table } => {
                    if cassie.catalog.exists(table) {
                        return Err(QueryError::General(format!(
                            "collection '{table}' already exists"
                        )));
                    }

                    cassie
                        .midge
                        .rename_collection(&statement.table, table)
                        .map_err(|error| QueryError::General(error.to_string()))?;
                    cassie
                        .catalog
                        .rename_collection(&statement.table, table)
                        ;
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
            if statement.if_not_exists && cassie.catalog.namespace_exists(&statement.schema) {
                return Ok(QueryResult {
                    columns: Vec::new(),
                    rows: Vec::new(),
                    command: "CREATE SCHEMA".to_string(),
                });
            }

            cassie
                .midge
                .create_namespace(&statement.schema)
                .map_err(|error| QueryError::General(error.to_string()))?;
            cassie
                .catalog
                .register_namespace(&statement.schema, None)
                ;
            invalidate_plan_cache = true;

            Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "CREATE SCHEMA".to_string(),
            })
        }
        LogicalCommand::DropSchema(statement) => {
            if statement.if_exists && !cassie.catalog.namespace_exists(&statement.schema) {
                return Ok(QueryResult {
                    columns: Vec::new(),
                    rows: Vec::new(),
                    command: "DROP SCHEMA".to_string(),
                });
            }

            cassie
                .midge
                .drop_namespace(&statement.schema)
                .map_err(|error| QueryError::General(error.to_string()))?;
            cassie.catalog.unregister_namespace(&statement.schema);
            invalidate_plan_cache = true;

            Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "DROP SCHEMA".to_string(),
            })
        }
        LogicalCommand::AlterSchema(statement) => {
            let next_schema = match &statement.operation {
                crate::sql::ast::AlterSchemaOperation::RenameTo { schema } => schema.clone(),
            };
            let target_schema = statement.schema.clone();

            if cassie.catalog.namespace_exists(&next_schema) {
                return Err(QueryError::General(format!(
                    "namespace '{next_schema}' already exists"
                )));
            };

            cassie
                .midge
                .rename_namespace(&target_schema, &next_schema)
                .map_err(|error| QueryError::General(error.to_string()))?;
            cassie
                .catalog
                .rename_namespace(&target_schema, &next_schema)
                ;
            invalidate_plan_cache = true;

            Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "ALTER SCHEMA".to_string(),
            })
        }
        LogicalCommand::CreateRole(statement) => {
            cassie
                .create_role(
                    &statement.name,
                    statement.login,
                    statement.password.clone(),
                    statement.if_not_exists,
                )
                
                .map_err(|error| QueryError::General(error.to_string()))?;
            invalidate_plan_cache = true;

            Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "CREATE ROLE".to_string(),
            })
        }
        LogicalCommand::AlterRole(statement) => {
            cassie
                .alter_role(&statement.name, statement.login, statement.password.clone())
                
                .map_err(|error| QueryError::General(error.to_string()))?;
            invalidate_plan_cache = true;

            Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "ALTER ROLE".to_string(),
            })
        }
        LogicalCommand::DropRole(statement) => {
            cassie
                .drop_role(&statement.name, statement.if_exists)
                
                .map_err(|error| QueryError::General(error.to_string()))?;
            invalidate_plan_cache = true;

            Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "DROP ROLE".to_string(),
            })
        }
        LogicalCommand::CreateIndex(statement) => {
            if matches!(statement.kind, catalog::IndexKind::Vector) {
                let vector_index = vector_index_metadata(cassie, statement)?;

                cassie
                    .midge
                    .put_vector_index(vector_index.clone())
                    .map_err(|error| QueryError::General(error.to_string()))?;
                cassie.catalog.register_vector_index(vector_index);
            }

            let metadata = catalog::IndexMeta {
                collection: statement.table.clone(),
                name: statement.name.clone(),
                field: statement.fields.first().cloned().unwrap_or_default(),
                fields: statement.fields.clone(),
                kind: statement.kind.clone(),
                unique: statement.unique,
                options: statement.options.clone(),
            };

            cassie
                .midge
                .put_index(metadata.clone())
                .map_err(|error| QueryError::General(error.to_string()))?;
            cassie.catalog.register_index(metadata);
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
                ;

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
                        .map_err(|error| QueryError::General(error.to_string()))?;
                    cassie
                        .catalog
                        .unregister_vector_index(&statement.table, &index.field)
                        ;
                }
            }

            cassie
                .midge
                .delete_index(&statement.table, &statement.name)
                .map_err(|error| QueryError::General(error.to_string()))?;
            cassie
                .catalog
                .unregister_index(&statement.table, &statement.name)
                ;
            invalidate_plan_cache = true;

            Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "DROP INDEX".to_string(),
            })
        }
        LogicalCommand::CreateFunction(statement) => {
            if statement.if_not_exists
                && cassie.catalog.get_function(&statement.name).is_some()
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
                .map_err(|error| QueryError::General(error.to_string()))?;
            cassie.catalog.register_function(metadata);
            invalidate_plan_cache = true;

            Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "CREATE FUNCTION".to_string(),
            })
        }
        LogicalCommand::DropFunction(statement) => {
            if statement.if_exists && cassie.catalog.get_function(&statement.name).is_none() {
                return Ok(QueryResult {
                    columns: Vec::new(),
                    rows: Vec::new(),
                    command: "DROP FUNCTION".to_string(),
                });
            }

            cassie
                .midge
                .delete_function(&statement.name)
                .map_err(|error| QueryError::General(error.to_string()))?;
            cassie.catalog.unregister_function(&statement.name);
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
                .map_err(|error| QueryError::General(error.to_string()))?;
            cassie.catalog.register_procedure(metadata);
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
                .map_err(|error| QueryError::General(error.to_string()))?;
            cassie.catalog.unregister_procedure(&statement.name);
            invalidate_plan_cache = true;

            Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "DROP PROCEDURE".to_string(),
            })
        }
        LogicalCommand::CallProcedure(statement) => {
            let Some(metadata) = cassie.catalog.get_procedure(&statement.name) else {
                return Err(QueryError::General(format!(
                    "procedure '{}' does not exist",
                    statement.name
                )));
            };

            let call_session = session
                .cloned()
                .unwrap_or_else(|| CassieSession::new("postgres".to_string(), None));
            let empty_row = Vec::<(String, Value)>::new();
            let evaluated_args = statement
                .args
                .iter()
                .map(|expr| {
                    filter::evaluate_expr_value(
                        &empty_row,
                        expr,
                        params,
                        None,
                        user_functions,
                        Some(&call_session),
                        None,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;

            call_session
                .enter_procedure_call(&statement.name)
                
                .map_err(|error| QueryError::General(error.to_string()))?;
            let body_result = cassie
                .execute_sql_with_controls(
                    &call_session,
                    &metadata.body,
                    evaluated_args,
                    crate::runtime::ExecutionMode::SimpleQuery,
                    controls,
                )
                ;
            call_session.leave_procedure_call();
            body_result.map_err(|error| QueryError::General(error.to_string()))?;

            Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "CALL".to_string(),
            })
        }
    };

    if invalidate_plan_cache {
        cassie
            .bump_schema_epoch_and_invalidate_query_cache()
            .map_err(|error| QueryError::General(error.to_string()))?;
    }

    result
}

fn execute_insert(
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
        
        .ok_or_else(|| {
            QueryError::General(format!("collection '{}' not found", statement.table))
        })?;

    let source_rows =
        insert_source_rows(cassie, session, statement, params, user_functions, controls)?;
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
            
            .map_err(|error| QueryError::General(error.to_string()))?;

        if !statement.returning.is_empty() {
            let document = cassie
                .get_document_for_session(session, &statement.table, &row_id)
                
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

    let column_schema = cassie.catalog.get_schema(&statement.table);
    let columns =
        dml_returning_columns(&statement.returning, column_schema.as_ref(), user_functions);

    Ok(QueryResult {
        columns,
        rows: projected.into_iter().map(BatchRow::into_values).collect(),
        command: format!("INSERT 0 {inserted_count}"),
    })
}

fn insert_source_rows(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    statement: &crate::sql::ast::InsertStatement,
    params: &[Value],
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<Vec<Vec<Value>>, QueryError> {
    match &statement.source {
        InsertSource::Values(rows) => rows
            .iter()
            .map(|row| {
                row.iter()
                    .map(|expr| {
                        insert_expr_to_json(expr, params)
                            .map_err(QueryError::General)
                            .map(|value| json_to_value(&value))
                    })
                    .collect::<Result<Vec<_>, _>>()
            })
            .collect::<Result<Vec<_>, _>>(),
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
                distinct_on: select.distinct_on.clone(),
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
            ?;
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
        InsertSource::Values(rows) => rows.first().map_or(0, Vec::len),
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
        | Expr::Not { .. }
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

fn update_assignment_to_json(
    field: &str,
    value: &Value,
    schema: &CollectionSchema,
) -> serde_json::Value {
    if let Some(field_meta) = schema
        .fields
        .iter()
        .find(|candidate| candidate.name.eq_ignore_ascii_case(field))
    {
        if matches!(
            field_meta.data_type,
            DataType::SmallInt | DataType::Int | DataType::BigInt
        ) {
            if let Value::Float64(number) = value {
                if number.is_finite()
                    && number.fract() == 0.0
                    && *number >= i64::MIN as f64
                    && *number <= i64::MAX as f64
                {
                    return serde_json::Value::Number((*number as i64).into());
                }
            }
        }
    }

    value_to_json(value)
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

fn dml_returning_columns(
    returning: &[SelectItem],
    schema: Option<&CollectionSchema>,
    user_functions: &HashMap<String, FunctionMeta>,
) -> Vec<ColumnMeta> {
    let mut columns = aggregate::columns_from_projection(returning, schema, user_functions);
    if returning
        .iter()
        .any(|item| matches!(item, SelectItem::Wildcard))
    {
        for column in &mut columns {
            if column.name == "id" {
                column.name = "_id".to_string();
                break;
            }
        }
    }
    columns
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

fn execute_update(
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
        
        .ok_or_else(|| {
            QueryError::General(format!("collection '{}' not found", statement.table))
        })?;

    let batches = scan::scan(cassie, session, &statement.table)?;
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
            payload.insert(
                field.clone(),
                update_assignment_to_json(field, &value, &schema),
            );
        }

        let payload = cassie
            .prepare_document_write_for_session(
                session,
                &statement.table,
                serde_json::Value::Object(payload),
                true,
                Some(&row_id),
            )
            
            .map_err(|error| QueryError::General(error.to_string()))?;
        prepared_rows.push((row_id, payload));
    }

    let mut returning_rows = Vec::new();
    for (row_id, payload) in prepared_rows {
        cassie
            .put_prepared_document_for_session(session, &statement.table, row_id.clone(), payload)
            
            .map_err(|error| QueryError::General(error.to_string()))?;

        if !statement.returning.is_empty() {
            let document = cassie
                .get_document_for_session(session, &statement.table, &row_id)
                
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

    let column_schema = cassie.catalog.get_schema(&statement.table);
    let columns =
        dml_returning_columns(&statement.returning, column_schema.as_ref(), user_functions);

    Ok(QueryResult {
        columns,
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

fn execute_delete(
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
        
        .ok_or_else(|| {
            QueryError::General(format!("collection '{}' not found", statement.table))
        })?;

    let batches = scan::scan(cassie, session, &statement.table)?;
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

    let column_schema = cassie.catalog.get_schema(&statement.table);
    let columns =
        dml_returning_columns(&statement.returning, column_schema.as_ref(), user_functions);

    Ok(QueryResult {
        columns,
        rows: projected.into_iter().map(BatchRow::into_values).collect(),
        command: format!("DELETE {deleted_count}"),
    })
}

fn vector_index_metadata(
    cassie: &Cassie,
    statement: &crate::sql::ast::CreateIndexStatement,
) -> Result<VectorIndexRecord, QueryError> {
    let schema = cassie
        .midge
        .collection_schema(&statement.table)
        .ok_or_else(|| {
            QueryError::General(format!(
                "collection '{}' not found while creating vector index",
                statement.table
            ))
        })?;

    let vector_field = schema
        .fields
        .iter()
        .find(|field| {
            statement
                .fields
                .first()
                .is_some_and(|value| field.name == *value)
        })
        .ok_or_else(|| {
            let field = statement.fields.first().cloned().unwrap_or_default();
            QueryError::General(format!(
                "index field '{}' does not exist in collection '{}'",
                field, statement.table
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
    if cassie.embedding_provider.dimensions() != dimensions {
        return Err(QueryError::General(format!(
            "embedding dimension mismatch: field '{}' has {}, active provider '{}' model '{}' has {}",
            vector_field.name,
            dimensions,
            cassie.embedding_provider.provider_name(),
            cassie.embedding_provider.model_name(),
            cassie.embedding_provider.dimensions()
        )));
    }

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
        field: statement.fields.first().cloned().unwrap_or_default(),
        source_field,
        metadata,
    })
}

fn execute_plan(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    plan: &LogicalPlan,
    cte_context: &mut CteContext,
    user_functions: &HashMap<String, FunctionMeta>,
    params: &[Value],
    controls: &QueryExecutionControls,
) -> Result<Vec<BatchRow>, QueryError> {
    execute_plan_with_outer_row(
        cassie,
        session,
        plan,
        cte_context,
        user_functions,
        params,
        controls,
        None,
    )
    
}

#[allow(clippy::too_many_arguments)]
fn execute_plan_with_outer_row(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    plan: &LogicalPlan,
    cte_context: &mut CteContext,
    user_functions: &HashMap<String, FunctionMeta>,
    params: &[Value],
    controls: &QueryExecutionControls,
    outer_row: Option<&BatchRow>,
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
        ?;
        cte_context.insert(cte.name.to_ascii_lowercase(), rows);
    }

    if outer_row.is_none() {
        if let Some(rows) = execute_vector_distance_top_k(cassie, plan)? {
            return Ok(rows);
        }

        if let Some(rows) = execute_scored_search_top_k(cassie, session, plan)? {
            return Ok(rows);
        }

        if let Some(rows) = execute_ordered_column_top_k(cassie, plan)? {
            return Ok(rows);
        }

        if let Some(rows) =
            execute_projected_filtered_read(cassie, session, plan, user_functions, params, controls)
                ?
        {
            return Ok(rows);
        }
    }

    execute_source_query_with_outer_row(
        cassie,
        session,
        plan,
        cte_context,
        user_functions,
        params,
        controls,
        outer_row,
    )
    
}

fn execute_plan_with_execution_breakdown(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    plan: &LogicalPlan,
    cte_context: &mut CteContext,
    user_functions: &HashMap<String, FunctionMeta>,
    params: &[Value],
    controls: &QueryExecutionControls,
) -> Result<(Vec<BatchRow>, ExecutionBreakdownDurations), QueryError> {
    if let Some(output) = execute_projected_filtered_read_with_breakdown(
        cassie,
        session,
        plan,
        user_functions,
        params,
        controls,
    )
    ?
    {
        return Ok(output);
    }

    let started = Instant::now();
    let rows = execute_plan(
        cassie,
        session,
        plan,
        cte_context,
        user_functions,
        params,
        controls,
    )
    ?;
    let breakdown = ExecutionBreakdownDurations {
        scan: started.elapsed(),
        ..ExecutionBreakdownDurations::default()
    };
    Ok((rows, breakdown))
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
                ?
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
                ?
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
                    ?
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
}

fn execute_vector_distance_top_k(
    cassie: &Cassie,
    plan: &LogicalPlan,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    let Some(spec) = vector_distance_top_k_spec(plan) else {
        return Ok(None);
    };

    let candidates = cassie
        .midge
        .scan_rows_for_rebuild(
            &spec.collection,
            RowDecode::Projected(vec![spec.vector_field.clone()]),
        )
        .map_err(|error| QueryError::General(error.to_string()))?;
    let top_needed = spec.limit.saturating_add(spec.offset).max(1);
    let mut top = BinaryHeap::with_capacity(top_needed.saturating_add(1));

    for document in candidates {
        let vector = document
            .payload
            .get(&spec.vector_field)
            .and_then(vector_from_json)
            .unwrap_or_default();
        let score = if vector.len() == spec.query.len() && !vector.is_empty() {
            crate::vector::l2_distance(&vector, &spec.query)
        } else {
            f64::INFINITY
        };
        let candidate = SqlVectorCandidate {
            sort_value: match spec.direction {
                SortDirection::Asc => score,
                SortDirection::Desc => -score,
            },
            score,
            id: document.id,
        };
        if top.len() < top_needed {
            top.push(candidate);
        } else if let Some(worst) = top.peek() {
            if candidate.is_better_than(worst) {
                top.pop();
                top.push(candidate);
            }
        }
    }

    let mut ranked = top.into_vec();
    ranked.sort_by(compare_sql_vector_candidates);
    let rows = ranked
        .into_iter()
        .skip(spec.offset)
        .take(spec.limit)
        .map(|candidate| {
            BatchRow::new(vec![
                (spec.id_column.clone(), Value::String(candidate.id)),
                (spec.score_column.clone(), Value::Float64(candidate.score)),
            ])
        })
        .collect();
    Ok(Some(rows))
}

struct VectorDistanceTopKSpec {
    collection: String,
    vector_field: String,
    query: Vec<f32>,
    id_column: String,
    score_column: String,
    direction: SortDirection,
    limit: usize,
    offset: usize,
}

fn vector_distance_top_k_spec(plan: &LogicalPlan) -> Option<VectorDistanceTopKSpec> {
    if plan.command.is_some()
        || !plan.ctes.is_empty()
        || plan.distinct
        || !plan.distinct_on.is_empty()
        || plan.filter.is_some()
        || !plan.group_by.is_empty()
        || plan.having.is_some()
        || plan.set.is_some()
        || plan.order.len() != 1
        || plan.projection.len() != 2
    {
        return None;
    }

    let QuerySource::Collection(collection) = &plan.source else {
        return None;
    };
    let limit = usize::try_from(plan.limit?).ok()?.max(1);
    let offset = plan
        .offset
        .and_then(|offset| usize::try_from(offset).ok())
        .unwrap_or(0);

    let (id_column, function, score_column) =
        vector_distance_projection(plan.projection.as_slice())?;
    if !order_matches_vector_distance_score(&plan.order[0], function, &score_column) {
        return None;
    }

    let (vector_field, query) = vector_distance_args(function)?;
    Some(VectorDistanceTopKSpec {
        collection: collection.clone(),
        vector_field,
        query,
        id_column,
        score_column,
        direction: plan.order[0].direction.clone(),
        limit,
        offset,
    })
}

fn vector_distance_projection(
    projection: &[SelectItem],
) -> Option<(String, &FunctionCall, String)> {
    let SelectItem::Column { name, alias: _ } = &projection[0] else {
        return None;
    };
    if !name.eq_ignore_ascii_case("id") && !name.eq_ignore_ascii_case("_id") {
        return None;
    }
    let SelectItem::Function { function, alias } = &projection[1] else {
        return None;
    };
    if !function.name.eq_ignore_ascii_case("vector_distance") {
        return None;
    }
    Some((
        alias.clone().unwrap_or_else(|| name.clone()),
        function,
        alias.clone().unwrap_or_else(|| function.name.clone()),
    ))
}

fn order_matches_vector_distance_score(
    order: &crate::sql::ast::OrderExpr,
    function: &FunctionCall,
    score_column: &str,
) -> bool {
    match &order.expr {
        Expr::Column(column) => column.eq_ignore_ascii_case(score_column),
        Expr::Function(order_function) => {
            order_function.name.eq_ignore_ascii_case("vector_distance")
                && vector_distance_args(order_function) == vector_distance_args(function)
        }
        _ => false,
    }
}

fn vector_distance_args(function: &FunctionCall) -> Option<(String, Vec<f32>)> {
    if function.args.len() != 2 {
        return None;
    }
    let Expr::Column(vector_field) = &function.args[0] else {
        return None;
    };
    let Expr::StringLiteral(query) = &function.args[1] else {
        return None;
    };
    Some((vector_field.clone(), parse_vector_literal(query)?))
}

fn parse_vector_literal(value: &str) -> Option<Vec<f32>> {
    let values = serde_json::from_str::<Vec<f32>>(value).ok()?;
    if values.is_empty() {
        return None;
    }
    Some(values)
}

fn vector_from_json(value: &serde_json::Value) -> Option<Vec<f32>> {
    let values = value.as_array()?;
    let mut out = Vec::with_capacity(values.len());
    for value in values {
        out.push(value.as_f64()? as f32);
    }
    Some(out)
}

#[derive(Debug, Clone, PartialEq)]
struct SqlVectorCandidate {
    sort_value: f64,
    score: f64,
    id: String,
}

impl SqlVectorCandidate {
    fn is_better_than(&self, other: &Self) -> bool {
        compare_sql_vector_candidates(self, other) == CmpOrdering::Less
    }
}

impl Eq for SqlVectorCandidate {}

impl PartialOrd for SqlVectorCandidate {
    fn partial_cmp(&self, other: &Self) -> Option<CmpOrdering> {
        Some(self.cmp(other))
    }
}

impl Ord for SqlVectorCandidate {
    fn cmp(&self, other: &Self) -> CmpOrdering {
        compare_sql_vector_candidates(self, other)
    }
}

fn compare_sql_vector_candidates(
    left: &SqlVectorCandidate,
    right: &SqlVectorCandidate,
) -> CmpOrdering {
    left.sort_value
        .total_cmp(&right.sort_value)
        .then_with(|| left.id.cmp(&right.id))
}

fn execute_scored_search_top_k(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    plan: &LogicalPlan,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    if let Some(spec) = fulltext_top_k_spec(plan) {
        return execute_fulltext_top_k(cassie, spec).map(Some);
    }
    if let Some(spec) = hybrid_top_k_spec(plan) {
        return execute_hybrid_top_k(cassie, spec).map(Some);
    }
    if let Some(spec) = fulltext_filtered_read_spec(plan) {
        if virtual_views::schema(&spec.collection).is_some()
            || cassie.catalog.get_view(&spec.collection).is_some()
        {
            return Ok(None);
        }
        return execute_fulltext_filtered_read(cassie, session, spec)
            
            .map(Some);
    }
    Ok(None)
}

struct TokenizedFulltextDocument {
    id: String,
    text_stats: filter::SearchTermStats,
}

struct TokenizedFulltextReadDocument {
    id: String,
    payload: serde_json::Value,
    text_stats: filter::SearchTermStats,
}

struct TokenizedHybridDocument {
    id: String,
    text_stats: filter::SearchTermStats,
    vector: Option<Vec<f32>>,
}

trait PostingListDocument {
    fn doc_id(&self) -> &str;
    fn term_stats(&self) -> &filter::SearchTermStats;
    fn term_counts(&self) -> &HashMap<String, usize>;
}

impl PostingListDocument for TokenizedFulltextDocument {
    fn doc_id(&self) -> &str {
        &self.id
    }

    fn term_stats(&self) -> &filter::SearchTermStats {
        &self.text_stats
    }

    fn term_counts(&self) -> &HashMap<String, usize> {
        self.text_stats.term_counts()
    }
}

impl PostingListDocument for TokenizedFulltextReadDocument {
    fn doc_id(&self) -> &str {
        &self.id
    }

    fn term_stats(&self) -> &filter::SearchTermStats {
        &self.text_stats
    }

    fn term_counts(&self) -> &HashMap<String, usize> {
        self.text_stats.term_counts()
    }
}

impl PostingListDocument for TokenizedHybridDocument {
    fn doc_id(&self) -> &str {
        &self.id
    }

    fn term_stats(&self) -> &filter::SearchTermStats {
        &self.text_stats
    }

    fn term_counts(&self) -> &HashMap<String, usize> {
        self.text_stats.term_counts()
    }
}

fn posting_list_candidate_ids<D>(documents: &[D], query_terms: &[String]) -> HashSet<String>
where
    D: PostingListDocument,
{
    if query_terms.is_empty() {
        return HashSet::new();
    }

    let mut index = crate::search::inverted_index::InvertedIndex::default();
    for document in documents {
        index.index_term_counts(document.doc_id(), document.term_counts());
    }
    index.candidate_documents(query_terms)
}

fn cached_search_context<D>(
    cassie: &Cassie,
    collection: &str,
    field: &str,
    documents: &[D],
    field_boost: &HashMap<String, f64>,
    field_k1: &HashMap<String, f64>,
    field_b: &HashMap<String, f64>,
) -> Result<filter::SearchContext, QueryError>
where
    D: PostingListDocument,
{
    let schema_epoch = cassie.runtime.schema_epoch();
    let data_epoch = cassie.runtime.data_epoch();
    if let Some(context) = query_cache::lookup_fulltext_stats(
        &cassie.midge,
        &cassie.runtime,
        collection,
        field,
        schema_epoch,
        data_epoch,
    )
    .map_err(|error| QueryError::General(error.to_string()))?
    {
        return Ok(context);
    }

    let context = filter::SearchContext::from_term_stats(
        field,
        documents.iter().map(|document| document.term_stats()),
        field_boost,
        field_k1,
        field_b,
    );
    query_cache::store_fulltext_stats(
        &cassie.midge,
        &cassie.runtime,
        collection,
        field,
        schema_epoch,
        data_epoch,
        &context,
    )
    .map_err(|error| QueryError::General(error.to_string()))?;
    Ok(context)
}

fn execute_fulltext_top_k(
    cassie: &Cassie,
    spec: FulltextTopKSpec,
) -> Result<Vec<BatchRow>, QueryError> {
    let started_at = Instant::now();
    let documents = cassie
        .midge
        .scan_rows_for_rebuild(
            &spec.collection,
            RowDecode::Projected(vec![spec.text_field.clone()]),
        )
        .map_err(|error| QueryError::General(error.to_string()))?;
    let search_documents = documents
        .into_iter()
        .map(|document| TokenizedFulltextDocument {
            id: document.id,
            text_stats: json_search_term_stats(document.payload.get(&spec.text_field)),
        })
        .collect::<Vec<_>>();
    let search_index_options = search_context_for_fields(
        cassie,
        &spec.collection,
        std::slice::from_ref(&spec.text_field),
    )
    ?;
    let search_context = cached_search_context(
        cassie,
        &spec.collection,
        &spec.text_field,
        &search_documents,
        &search_index_options.field_boost,
        &search_index_options.field_k1,
        &search_index_options.field_b,
    )
    ?;
    let query_terms = filter::prepare_query_terms(&spec.query);
    let candidate_ids = if spec.require_match {
        Some(posting_list_candidate_ids(&search_documents, &query_terms))
    } else {
        None
    };
    let mut top = BinaryHeap::with_capacity(spec.top_needed().saturating_add(1));

    for document in &search_documents {
        if let Some(candidate_ids) = candidate_ids.as_ref() {
            if !candidate_ids.contains(document.id.as_str()) {
                continue;
            }
        }
        let score = search_context.score_term_stats(
            Some(&spec.text_field),
            &document.text_stats,
            &query_terms,
        );
        if spec.require_match && score == 0.0 {
            continue;
        }
        let candidate = ScoredSearchCandidate {
            sort_value: -score,
            score,
            id: document.id.clone(),
        };
        push_top_k(&mut top, spec.top_needed(), candidate);
    }

    let rows = scored_candidates_to_rows(
        top,
        spec.offset,
        spec.limit,
        &spec.id_column,
        &spec.score_column,
    );
    let candidate_count = candidate_ids
        .as_ref()
        .map_or(search_documents.len(), HashSet::len);
    cassie
        .runtime
        .record_search_execution(started_at.elapsed(), candidate_count, rows.len());
    Ok(rows)
}

fn execute_hybrid_top_k(
    cassie: &Cassie,
    spec: HybridTopKSpec,
) -> Result<Vec<BatchRow>, QueryError> {
    let started_at = Instant::now();
    let documents = cassie
        .midge
        .scan_rows_for_rebuild(
            &spec.collection,
            RowDecode::Projected(vec![spec.text_field.clone(), spec.vector_field.clone()]),
        )
        .map_err(|error| QueryError::General(error.to_string()))?;
    let search_documents = documents
        .into_iter()
        .map(|document| TokenizedHybridDocument {
            id: document.id,
            text_stats: json_search_term_stats(document.payload.get(&spec.text_field)),
            vector: document
                .payload
                .get(&spec.vector_field)
                .and_then(vector_from_json),
        })
        .collect::<Vec<_>>();
    let search_index_options = search_context_for_fields(
        cassie,
        &spec.collection,
        std::slice::from_ref(&spec.text_field),
    )
    ?;
    let search_context = cached_search_context(
        cassie,
        &spec.collection,
        &spec.text_field,
        &search_documents,
        &search_index_options.field_boost,
        &search_index_options.field_k1,
        &search_index_options.field_b,
    )
    ?;
    let query_terms = filter::prepare_query_terms(&spec.query);
    let candidate_ids = posting_list_candidate_ids(&search_documents, &query_terms);
    let mut top = BinaryHeap::with_capacity(spec.top_needed().saturating_add(1));
    let mut text_candidate_count = 0usize;

    for document in &search_documents {
        if !candidate_ids.contains(document.id.as_str()) {
            continue;
        }
        let search_score = search_context.score_term_stats(
            Some(&spec.text_field),
            &document.text_stats,
            &query_terms,
        );
        if search_score == 0.0 {
            continue;
        }
        text_candidate_count += 1;
        let vector = document.vector.as_ref().ok_or_else(|| {
            QueryError::General("vector_score expects vector in first argument".to_string())
        })?;
        if vector.len() != spec.vector_query.len() {
            return Err(QueryError::General(format!(
                "vector_score vector length mismatch: {} != {}",
                vector.len(),
                spec.vector_query.len()
            )));
        }
        let vector_score = 1.0 / (1.0 + crate::vector::l2_distance(vector, &spec.vector_query));
        let score = crate::hybrid::hybrid_score(search_score, vector_score, None);
        let candidate = ScoredSearchCandidate {
            sort_value: -score,
            score,
            id: document.id.clone(),
        };
        push_top_k(&mut top, spec.top_needed(), candidate);
    }

    let rows = scored_candidates_to_rows(
        top,
        spec.offset,
        spec.limit,
        &spec.id_column,
        &spec.score_column,
    );
    let candidate_count = candidate_ids.len();
    cassie
        .runtime
        .record_search_execution(started_at.elapsed(), candidate_count, rows.len());
    cassie
        .runtime
        .record_vector_execution(started_at.elapsed(), text_candidate_count, rows.len());
    cassie
        .runtime
        .record_hybrid_execution(started_at.elapsed(), text_candidate_count, rows.len());
    Ok(rows)
}

fn execute_fulltext_filtered_read(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    spec: FulltextFilteredReadSpec,
) -> Result<Vec<BatchRow>, QueryError> {
    let started_at = Instant::now();
    let scan_fields = fulltext_filtered_scan_fields(&spec);
    let document_batches = cassie
        .scan_projected_documents_batched_for_session(
            session,
            &spec.collection,
            batch::DEFAULT_BATCH_SIZE,
            &scan_fields,
            None,
        )
        
        .map_err(|error| QueryError::General(error.to_string()))?;
    let search_documents = document_batches
        .into_iter()
        .flat_map(|documents| documents.into_iter())
        .map(|document| TokenizedFulltextReadDocument {
            id: document.id,
            text_stats: json_search_term_stats(json_projected_value(
                &document.payload,
                &spec.text_field,
            )),
            payload: document.payload,
        })
        .collect::<Vec<_>>();
    let search_index_options = search_context_for_fields(
        cassie,
        &spec.collection,
        std::slice::from_ref(&spec.text_field),
    )
    ?;
    let search_context = cached_search_context(
        cassie,
        &spec.collection,
        &spec.text_field,
        &search_documents,
        &search_index_options.field_boost,
        &search_index_options.field_k1,
        &search_index_options.field_b,
    )
    ?;
    let query_terms = filter::prepare_query_terms(&spec.query);
    let candidate_ids = posting_list_candidate_ids(&search_documents, &query_terms);

    let mut skipped = 0usize;
    let mut rows = Vec::new();
    for document in &search_documents {
        if !candidate_ids.contains(document.id.as_str()) {
            continue;
        }
        let score = search_context.score_term_stats(
            Some(&spec.text_field),
            &document.text_stats,
            &query_terms,
        );
        if score == 0.0 {
            continue;
        }
        if skipped < spec.offset {
            skipped += 1;
            continue;
        }
        if let Some(limit) = spec.limit {
            if rows.len() >= limit {
                break;
            }
        }

        let mut entries = Vec::with_capacity(spec.columns.len().saturating_add(1));
        for column in &spec.columns {
            let value = if is_row_id_column(&column.name) {
                Value::String(document.id.clone())
            } else {
                json_projected_value(&document.payload, &column.name)
                    .map(json_to_query_value)
                    .unwrap_or(Value::Null)
            };
            entries.push((column.output_name.clone(), value));
        }
        entries.push((spec.score_column.clone(), Value::Float64(score)));
        rows.push(BatchRow::new(entries));
    }

    let candidate_count = candidate_ids.len();
    cassie
        .runtime
        .record_search_execution(started_at.elapsed(), candidate_count, rows.len());
    Ok(rows)
}

fn search_context_for_fields(
    cassie: &Cassie,
    collection: &str,
    fields: &[String],
) -> Result<FulltextIndexOptions, QueryError> {
    let requested_fields = fields
        .iter()
        .map(|field| field.to_ascii_lowercase())
        .collect::<HashSet<_>>();
    load_fulltext_index_options(cassie, collection, &requested_fields)
}

struct FulltextTopKSpec {
    collection: String,
    text_field: String,
    query: String,
    id_column: String,
    score_column: String,
    require_match: bool,
    limit: usize,
    offset: usize,
}

impl FulltextTopKSpec {
    fn top_needed(&self) -> usize {
        self.limit.saturating_add(self.offset).max(1)
    }
}

struct SearchProjectionColumn {
    name: String,
    output_name: String,
}

struct FulltextFilteredReadSpec {
    collection: String,
    text_field: String,
    query: String,
    columns: Vec<SearchProjectionColumn>,
    score_column: String,
    limit: Option<usize>,
    offset: usize,
}

struct HybridTopKSpec {
    collection: String,
    text_field: String,
    query: String,
    vector_field: String,
    vector_query: Vec<f32>,
    id_column: String,
    score_column: String,
    limit: usize,
    offset: usize,
}

impl HybridTopKSpec {
    fn top_needed(&self) -> usize {
        self.limit.saturating_add(self.offset).max(1)
    }
}

fn fulltext_top_k_spec(plan: &LogicalPlan) -> Option<FulltextTopKSpec> {
    if !simple_scored_top_k_plan(plan) {
        return None;
    }
    let QuerySource::Collection(collection) = &plan.source else {
        return None;
    };
    let limit = usize::try_from(plan.limit?).ok()?.max(1);
    let offset = plan
        .offset
        .and_then(|offset| usize::try_from(offset).ok())
        .unwrap_or(0);
    let (id_column, function, score_column) =
        scored_projection(plan.projection.as_slice(), "search_score")?;
    if !order_matches_function_score(&plan.order[0], function, &score_column) {
        return None;
    }
    let (text_field, query) = search_function_args(function)?;
    let require_match = match &plan.filter {
        None => false,
        Some(Expr::Function(filter)) => {
            let (filter_field, filter_query) = search_predicate_args(filter)?;
            filter_field.eq_ignore_ascii_case(&text_field) && filter_query == query
        }
        _ => return None,
    };
    if plan.filter.is_some() && !require_match {
        return None;
    }

    Some(FulltextTopKSpec {
        collection: collection.clone(),
        text_field,
        query,
        id_column,
        score_column,
        require_match,
        limit,
        offset,
    })
}

fn fulltext_filtered_read_spec(plan: &LogicalPlan) -> Option<FulltextFilteredReadSpec> {
    if plan.command.is_some()
        || !plan.ctes.is_empty()
        || plan.distinct
        || !plan.distinct_on.is_empty()
        || !plan.group_by.is_empty()
        || plan.having.is_some()
        || plan.set.is_some()
        || !plan.order.is_empty()
    {
        return None;
    }
    let QuerySource::Collection(collection) = &plan.source else {
        return None;
    };
    let (columns, function, score_column) =
        fulltext_filtered_projection(plan.projection.as_slice())?;
    let (text_field, query) = search_function_args(function)?;
    let filter = plan.filter.as_ref()?;
    let Expr::Function(filter_function) = filter else {
        return None;
    };
    let (filter_field, filter_query) = search_predicate_args(filter_function)?;
    if !filter_field.eq_ignore_ascii_case(&text_field) || filter_query != query {
        return None;
    }

    let limit = if let Some(limit) = plan.limit {
        Some(usize::try_from(limit.max(0)).ok()?)
    } else {
        None
    };
    let offset = plan
        .offset
        .and_then(|offset| usize::try_from(offset.max(0)).ok())
        .unwrap_or(0);

    Some(FulltextFilteredReadSpec {
        collection: collection.clone(),
        text_field,
        query,
        columns,
        score_column,
        limit,
        offset,
    })
}

fn fulltext_filtered_projection(
    projection: &[SelectItem],
) -> Option<(Vec<SearchProjectionColumn>, &FunctionCall, String)> {
    let (last, columns) = projection.split_last()?;
    let SelectItem::Function {
        function,
        alias: score_alias,
    } = last
    else {
        return None;
    };
    if !function.name.eq_ignore_ascii_case("search_score") {
        return None;
    }
    let columns = columns
        .iter()
        .map(|item| match item {
            SelectItem::Column { name, alias } => Some(SearchProjectionColumn {
                name: name.clone(),
                output_name: alias.clone().unwrap_or_else(|| name.clone()),
            }),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    if columns.is_empty() {
        return None;
    }

    Some((
        columns,
        function,
        score_alias.clone().unwrap_or_else(|| function.name.clone()),
    ))
}

fn fulltext_filtered_scan_fields(spec: &FulltextFilteredReadSpec) -> Vec<String> {
    let mut fields = vec![spec.text_field.clone()];
    for column in &spec.columns {
        if is_row_id_column(&column.name)
            || fields
                .iter()
                .any(|field| field.eq_ignore_ascii_case(&column.name))
        {
            continue;
        }
        fields.push(column.name.clone());
    }
    fields
}

fn hybrid_top_k_spec(plan: &LogicalPlan) -> Option<HybridTopKSpec> {
    if !simple_scored_top_k_plan(plan) || plan.filter.is_some() {
        return None;
    }
    let QuerySource::Collection(collection) = &plan.source else {
        return None;
    };
    let limit = usize::try_from(plan.limit?).ok()?.max(1);
    let offset = plan
        .offset
        .and_then(|offset| usize::try_from(offset).ok())
        .unwrap_or(0);
    let (id_column, function, score_column) =
        scored_projection(plan.projection.as_slice(), "hybrid_score")?;
    if !order_matches_function_score(&plan.order[0], function, &score_column) {
        return None;
    }
    let (text_field, query, vector_field, vector_query) = hybrid_function_args(function)?;

    Some(HybridTopKSpec {
        collection: collection.clone(),
        text_field,
        query,
        vector_field,
        vector_query,
        id_column,
        score_column,
        limit,
        offset,
    })
}

fn simple_scored_top_k_plan(plan: &LogicalPlan) -> bool {
    plan.command.is_none()
        && plan.ctes.is_empty()
        && !plan.distinct
        && plan.distinct_on.is_empty()
        && plan.group_by.is_empty()
        && plan.having.is_none()
        && plan.set.is_none()
        && plan.order.len() == 1
        && matches!(plan.order[0].direction, SortDirection::Desc)
        && plan.order[0].nulls.is_none()
        && plan.projection.len() == 2
}

fn scored_projection<'a>(
    projection: &'a [SelectItem],
    function_name: &str,
) -> Option<(String, &'a FunctionCall, String)> {
    let SelectItem::Column { name, alias } = &projection[0] else {
        return None;
    };
    if !name.eq_ignore_ascii_case("id") && !name.eq_ignore_ascii_case("_id") {
        return None;
    }
    let SelectItem::Function {
        function,
        alias: score_alias,
    } = &projection[1]
    else {
        return None;
    };
    if !function.name.eq_ignore_ascii_case(function_name) {
        return None;
    }
    Some((
        alias.clone().unwrap_or_else(|| name.clone()),
        function,
        score_alias.clone().unwrap_or_else(|| function.name.clone()),
    ))
}

fn order_matches_function_score(
    order: &crate::sql::ast::OrderExpr,
    function: &FunctionCall,
    score_column: &str,
) -> bool {
    match &order.expr {
        Expr::Column(column) => column.eq_ignore_ascii_case(score_column),
        Expr::Function(order_function) => {
            function_call_key(order_function) == function_call_key(function)
        }
        _ => false,
    }
}

fn search_function_args(function: &FunctionCall) -> Option<(String, String)> {
    if !function.name.eq_ignore_ascii_case("search_score") || function.args.len() != 2 {
        return None;
    }
    let Expr::Column(field) = &function.args[0] else {
        return None;
    };
    let Expr::StringLiteral(query) = &function.args[1] else {
        return None;
    };
    Some((field.clone(), query.clone()))
}

fn search_predicate_args(function: &FunctionCall) -> Option<(String, String)> {
    if !matches!(
        function.name.to_ascii_lowercase().as_str(),
        "search" | "search_score"
    ) || function.args.len() != 2
    {
        return None;
    }
    let Expr::Column(field) = &function.args[0] else {
        return None;
    };
    let Expr::StringLiteral(query) = &function.args[1] else {
        return None;
    };
    Some((field.clone(), query.clone()))
}

fn hybrid_function_args(function: &FunctionCall) -> Option<(String, String, String, Vec<f32>)> {
    if !function.name.eq_ignore_ascii_case("hybrid_score") || function.args.len() != 2 {
        return None;
    }
    let Expr::Function(search_function) = &function.args[0] else {
        return None;
    };
    let Expr::Function(vector_function) = &function.args[1] else {
        return None;
    };
    let (text_field, query) = search_function_args(search_function)?;
    let (vector_field, vector_query) = vector_score_args(vector_function)?;
    Some((text_field, query, vector_field, vector_query))
}

fn vector_score_args(function: &FunctionCall) -> Option<(String, Vec<f32>)> {
    if !function.name.eq_ignore_ascii_case("vector_score") || function.args.len() != 2 {
        return None;
    }
    let Expr::Column(field) = &function.args[0] else {
        return None;
    };
    let Expr::StringLiteral(query) = &function.args[1] else {
        return None;
    };
    Some((field.clone(), parse_vector_literal(query)?))
}

fn function_call_key(function: &FunctionCall) -> String {
    let args = function
        .args
        .iter()
        .map(expr_key)
        .collect::<Vec<_>>()
        .join(",");
    format!("{}({})", function.name.to_ascii_lowercase(), args)
}

fn json_search_term_stats(value: Option<&serde_json::Value>) -> filter::SearchTermStats {
    filter::SearchTermStats::from_text(value.and_then(serde_json::Value::as_str))
}

fn json_projected_value<'a>(
    payload: &'a serde_json::Value,
    field: &str,
) -> Option<&'a serde_json::Value> {
    payload
        .as_object()?
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(field))
        .map(|(_, value)| value)
}

#[derive(Debug, Clone, PartialEq)]
struct ScoredSearchCandidate {
    sort_value: f64,
    score: f64,
    id: String,
}

impl ScoredSearchCandidate {
    fn is_better_than(&self, other: &Self) -> bool {
        compare_scored_search_candidates(self, other) == CmpOrdering::Less
    }
}

impl Eq for ScoredSearchCandidate {}

impl PartialOrd for ScoredSearchCandidate {
    fn partial_cmp(&self, other: &Self) -> Option<CmpOrdering> {
        Some(self.cmp(other))
    }
}

impl Ord for ScoredSearchCandidate {
    fn cmp(&self, other: &Self) -> CmpOrdering {
        compare_scored_search_candidates(self, other)
    }
}

fn compare_scored_search_candidates(
    left: &ScoredSearchCandidate,
    right: &ScoredSearchCandidate,
) -> CmpOrdering {
    left.sort_value
        .total_cmp(&right.sort_value)
        .then_with(|| left.id.cmp(&right.id))
}

fn push_top_k(
    top: &mut BinaryHeap<ScoredSearchCandidate>,
    top_needed: usize,
    candidate: ScoredSearchCandidate,
) {
    if top.len() < top_needed {
        top.push(candidate);
    } else if let Some(worst) = top.peek() {
        if candidate.is_better_than(worst) {
            top.pop();
            top.push(candidate);
        }
    }
}

fn scored_candidates_to_rows(
    top: BinaryHeap<ScoredSearchCandidate>,
    offset: usize,
    limit: usize,
    id_column: &str,
    score_column: &str,
) -> Vec<BatchRow> {
    let mut ranked = top.into_vec();
    ranked.sort_by(compare_scored_search_candidates);
    ranked
        .into_iter()
        .skip(offset)
        .take(limit)
        .map(|candidate| {
            BatchRow::new(vec![
                (id_column.to_string(), Value::String(candidate.id)),
                (score_column.to_string(), Value::Float64(candidate.score)),
            ])
        })
        .collect()
}

fn execute_ordered_column_top_k(
    cassie: &Cassie,
    plan: &LogicalPlan,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    let Some(spec) = ordered_column_top_k_spec(plan) else {
        return Ok(None);
    };

    let documents = cassie
        .midge
        .scan_rows_for_rebuild(
            &spec.collection,
            RowDecode::Projected(spec.projected_scan_fields()),
        )
        .map_err(|error| QueryError::General(error.to_string()))?;
    let mut top = BinaryHeap::with_capacity(spec.top_needed().saturating_add(1));

    for document in documents {
        let order_value = if is_row_id_column(&spec.order_column) {
            Value::String(document.id.clone())
        } else {
            document
                .payload
                .get(&spec.order_column)
                .map(json_to_query_value)
                .unwrap_or(Value::Null)
        };
        let values = spec
            .projection
            .iter()
            .map(|column| {
                let value = if is_row_id_column(&column.name) {
                    Value::String(document.id.clone())
                } else {
                    document
                        .payload
                        .get(&column.name)
                        .map(json_to_query_value)
                        .unwrap_or(Value::Null)
                };
                (column.output_name.clone(), value)
            })
            .collect();
        let candidate = OrderedColumnCandidate {
            order_value,
            id: document.id,
            values,
            direction: spec.direction.clone(),
        };
        push_ordered_column_top_k(&mut top, spec.top_needed(), candidate);
    }

    let mut ranked = top.into_vec();
    ranked.sort_by(compare_ordered_column_candidates);
    let rows = ranked
        .into_iter()
        .skip(spec.offset)
        .take(spec.limit)
        .map(|candidate| BatchRow::new(candidate.values))
        .collect();
    Ok(Some(rows))
}

struct OrderedColumnTopKSpec {
    collection: String,
    order_column: String,
    direction: SortDirection,
    projection: Vec<OrderedProjectionColumn>,
    limit: usize,
    offset: usize,
}

impl OrderedColumnTopKSpec {
    fn top_needed(&self) -> usize {
        self.limit.saturating_add(self.offset).max(1)
    }

    fn projected_scan_fields(&self) -> Vec<String> {
        let mut fields = Vec::new();
        if !is_row_id_column(&self.order_column) {
            fields.push(self.order_column.clone());
        }
        for column in &self.projection {
            if !is_row_id_column(&column.name) && !fields.contains(&column.name) {
                fields.push(column.name.clone());
            }
        }
        fields
    }
}

struct OrderedProjectionColumn {
    name: String,
    output_name: String,
}

fn ordered_column_top_k_spec(plan: &LogicalPlan) -> Option<OrderedColumnTopKSpec> {
    if plan.command.is_some()
        || !plan.ctes.is_empty()
        || plan.distinct
        || !plan.distinct_on.is_empty()
        || plan.filter.is_some()
        || !plan.group_by.is_empty()
        || plan.having.is_some()
        || plan.set.is_some()
        || plan.order.len() != 1
        || plan.order[0].nulls.is_some()
    {
        return None;
    }

    let QuerySource::Collection(collection) = &plan.source else {
        return None;
    };
    let limit = usize::try_from(plan.limit?).ok()?.max(1);
    let offset = plan
        .offset
        .and_then(|offset| usize::try_from(offset).ok())
        .unwrap_or(0);
    let Expr::Column(order_column) = &plan.order[0].expr else {
        return None;
    };
    let projection = plan
        .projection
        .iter()
        .map(|item| match item {
            SelectItem::Column { name, alias } => Some(OrderedProjectionColumn {
                name: name.clone(),
                output_name: alias.clone().unwrap_or_else(|| name.clone()),
            }),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    if projection.is_empty() {
        return None;
    }

    Some(OrderedColumnTopKSpec {
        collection: collection.clone(),
        order_column: order_column.clone(),
        direction: plan.order[0].direction.clone(),
        projection,
        limit,
        offset,
    })
}

#[derive(Debug, Clone)]
struct OrderedColumnCandidate {
    order_value: Value,
    id: String,
    values: Vec<(String, Value)>,
    direction: SortDirection,
}

impl OrderedColumnCandidate {
    fn is_better_than(&self, other: &Self) -> bool {
        compare_ordered_column_candidates(self, other) == CmpOrdering::Less
    }
}

impl PartialEq for OrderedColumnCandidate {
    fn eq(&self, other: &Self) -> bool {
        compare_ordered_column_candidates(self, other) == CmpOrdering::Equal
    }
}

impl Eq for OrderedColumnCandidate {}

impl PartialOrd for OrderedColumnCandidate {
    fn partial_cmp(&self, other: &Self) -> Option<CmpOrdering> {
        Some(self.cmp(other))
    }
}

impl Ord for OrderedColumnCandidate {
    fn cmp(&self, other: &Self) -> CmpOrdering {
        compare_ordered_column_candidates(self, other)
    }
}

fn compare_ordered_column_candidates(
    left: &OrderedColumnCandidate,
    right: &OrderedColumnCandidate,
) -> CmpOrdering {
    let value_order = compare_query_values(&left.order_value, &right.order_value);
    let value_order = match &left.direction {
        SortDirection::Asc => value_order,
        SortDirection::Desc => value_order.reverse(),
    };
    value_order.then_with(|| left.id.cmp(&right.id))
}

fn compare_query_values(left: &Value, right: &Value) -> CmpOrdering {
    if let (Some(left), Some(right)) = (left.as_f64(), right.as_f64()) {
        return left.partial_cmp(&right).unwrap_or(CmpOrdering::Equal);
    }
    if let (Some(left), Some(right)) = (left.as_str(), right.as_str()) {
        return left.cmp(right);
    }
    CmpOrdering::Equal
}

fn push_ordered_column_top_k(
    top: &mut BinaryHeap<OrderedColumnCandidate>,
    top_needed: usize,
    candidate: OrderedColumnCandidate,
) {
    if top.len() < top_needed {
        top.push(candidate);
    } else if let Some(worst) = top.peek() {
        if candidate.is_better_than(worst) {
            top.pop();
            top.push(candidate);
        }
    }
}

fn is_row_id_column(column: &str) -> bool {
    column.eq_ignore_ascii_case("id") || column.eq_ignore_ascii_case("_id")
}

fn json_to_query_value(value: &serde_json::Value) -> Value {
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

fn execute_projected_filtered_read(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    plan: &LogicalPlan,
    user_functions: &HashMap<String, FunctionMeta>,
    params: &[Value],
    controls: &QueryExecutionControls,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    let Some(spec) = projected_filtered_read_spec(plan) else {
        return Ok(None);
    };
    if virtual_views::schema(&spec.collection).is_some()
        || cassie.catalog.get_view(&spec.collection).is_some()
    {
        return Ok(None);
    }

    let pushdown_filter = plan
        .filter
        .as_ref()
        .and_then(projected_scan_pushdown_filter);
    let mut batches = scan::scan_projected_filtered(
        cassie,
        session,
        &spec.collection,
        &spec.scan_fields,
        spec.scan_limit,
        pushdown_filter.as_ref(),
    )
    ?;
    ensure_temp_budget(controls, &batches)?;

    if pushdown_filter.is_none() {
        if let Some(filter_expr) = &plan.filter {
            batches = filter::filter_batches(
                batches,
                filter_expr,
                params,
                None,
                user_functions,
                session,
            )?;
            ensure_temp_budget(controls, &batches)?;
        }
    }

    batches = projection::project_batches(
        batches,
        &plan.projection,
        params,
        None,
        user_functions,
        session,
    )?;
    ensure_temp_budget(controls, &batches)?;

    if let Some(offset) = plan.offset {
        let offset = offset.max(0) as usize;
        let limit = plan.limit.map(|value| value.max(0) as usize);
        batches = batch::slice_batches(batches, offset, limit);
    } else if let Some(limit) = plan.limit {
        let limit = limit.max(0) as usize;
        batches = batch::slice_batches(batches, 0, Some(limit));
    }

    Ok(Some(batch::flatten_batches(batches)))
}

fn execute_projected_filtered_read_with_breakdown(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    plan: &LogicalPlan,
    user_functions: &HashMap<String, FunctionMeta>,
    params: &[Value],
    controls: &QueryExecutionControls,
) -> Result<Option<(Vec<BatchRow>, ExecutionBreakdownDurations)>, QueryError> {
    let Some(spec) = projected_filtered_read_spec(plan) else {
        return Ok(None);
    };
    if virtual_views::schema(&spec.collection).is_some()
        || cassie.catalog.get_view(&spec.collection).is_some()
    {
        return Ok(None);
    }

    let mut breakdown = ExecutionBreakdownDurations::default();

    let scan_started = Instant::now();
    let pushdown_filter = plan
        .filter
        .as_ref()
        .and_then(projected_scan_pushdown_filter);
    let (mut batches, scan_timings) = scan::scan_projected_filtered_with_timings(
        cassie,
        session,
        &spec.collection,
        &spec.scan_fields,
        spec.scan_limit,
        pushdown_filter.as_ref(),
    )
    ?;
    breakdown.row_decode += scan_timings.row_decode;
    let measured_scan = scan_timings.scan.saturating_add(scan_timings.row_decode);
    breakdown.scan += scan_timings
        .scan
        .saturating_add(scan_started.elapsed().saturating_sub(measured_scan));
    ensure_temp_budget(controls, &batches)?;

    if pushdown_filter.is_none() {
        if let Some(filter_expr) = &plan.filter {
            let filter_started = Instant::now();
            batches = filter::filter_batches(
                batches,
                filter_expr,
                params,
                None,
                user_functions,
                session,
            )?;
            ensure_temp_budget(controls, &batches)?;
            breakdown.filter += filter_started.elapsed();
        }
    }

    let projection_started = Instant::now();
    batches = projection::project_batches(
        batches,
        &plan.projection,
        params,
        None,
        user_functions,
        session,
    )?;
    ensure_temp_budget(controls, &batches)?;
    breakdown.projection += projection_started.elapsed();

    let result_started = Instant::now();
    if let Some(offset) = plan.offset {
        let offset = offset.max(0) as usize;
        let limit = plan.limit.map(|value| value.max(0) as usize);
        batches = batch::slice_batches(batches, offset, limit);
    } else if let Some(limit) = plan.limit {
        let limit = limit.max(0) as usize;
        batches = batch::slice_batches(batches, 0, Some(limit));
    }
    let rows = batch::flatten_batches(batches);
    breakdown.result_build += result_started.elapsed();

    Ok(Some((rows, breakdown)))
}

struct ProjectedFilteredReadSpec {
    collection: String,
    scan_fields: Vec<String>,
    scan_limit: Option<usize>,
}

fn projected_filtered_read_spec(plan: &LogicalPlan) -> Option<ProjectedFilteredReadSpec> {
    if plan.command.is_some()
        || !plan.ctes.is_empty()
        || plan.distinct
        || !plan.distinct_on.is_empty()
        || !plan.group_by.is_empty()
        || plan.having.is_some()
        || plan.set.is_some()
        || !plan.order.is_empty()
    {
        return None;
    }

    let QuerySource::Collection(collection) = &plan.source else {
        return None;
    };
    let filter_columns = match plan.filter.as_ref() {
        Some(filter) => projected_scan_filter_columns(filter)?,
        None => Vec::new(),
    };
    let projection_columns = plan
        .projection
        .iter()
        .map(|item| match item {
            SelectItem::Column { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    if projection_columns.is_empty() {
        return None;
    }

    let mut scan_fields = Vec::new();
    for column in projection_columns.into_iter().chain(filter_columns) {
        if is_row_id_column(&column) || scan_fields.contains(&column) {
            continue;
        }
        scan_fields.push(column);
    }

    let scan_limit = if plan.filter.is_none() {
        projected_scan_limit(plan.limit, plan.offset)
    } else {
        None
    };

    Some(ProjectedFilteredReadSpec {
        collection: collection.clone(),
        scan_fields,
        scan_limit,
    })
}

fn projected_scan_limit(limit: Option<i64>, offset: Option<i64>) -> Option<usize> {
    let limit = limit?;
    let limit = usize::try_from(limit.max(0)).ok()?;
    let offset = usize::try_from(offset.unwrap_or(0).max(0)).ok()?;
    limit.checked_add(offset)
}

fn projected_scan_pushdown_filter(expr: &Expr) -> Option<scan::ProjectedDocumentFilter> {
    let Expr::Binary {
        left,
        op: BinaryOp::Eq,
        right,
    } = expr
    else {
        return None;
    };

    match (left.as_ref(), right.as_ref()) {
        (Expr::Column(field), literal) => {
            if is_row_id_column(field) {
                return None;
            }
            projected_pushdown_literal(literal).map(|value| scan::ProjectedDocumentFilter {
                field: field.clone(),
                value,
            })
        }
        (literal, Expr::Column(field)) => {
            if is_row_id_column(field) {
                return None;
            }
            projected_pushdown_literal(literal).map(|value| scan::ProjectedDocumentFilter {
                field: field.clone(),
                value,
            })
        }
        _ => None,
    }
}

fn projected_pushdown_literal(expr: &Expr) -> Option<Value> {
    match expr {
        Expr::StringLiteral(value) => Some(Value::String(value.clone())),
        Expr::BoolLiteral(value) => Some(Value::Bool(*value)),
        Expr::Null => Some(Value::Null),
        _ => None,
    }
}

fn projected_scan_filter_columns(expr: &Expr) -> Option<Vec<String>> {
    let mut fields = Vec::new();
    collect_projected_scan_filter_columns(expr, &mut fields)?;
    Some(fields)
}

fn collect_projected_scan_filter_columns(expr: &Expr, fields: &mut Vec<String>) -> Option<()> {
    match expr {
        Expr::Column(name) => {
            if !fields.iter().any(|field| field.eq_ignore_ascii_case(name)) {
                fields.push(name.clone());
            }
            Some(())
        }
        Expr::Param(_)
        | Expr::StringLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::BoolLiteral(_)
        | Expr::Null => Some(()),
        Expr::Binary {
            left, op, right, ..
        } => {
            match op {
                crate::sql::ast::BinaryOp::Eq
                | crate::sql::ast::BinaryOp::NotEq
                | crate::sql::ast::BinaryOp::Lt
                | crate::sql::ast::BinaryOp::Lte
                | crate::sql::ast::BinaryOp::Gt
                | crate::sql::ast::BinaryOp::Gte
                | crate::sql::ast::BinaryOp::And
                | crate::sql::ast::BinaryOp::Or
                | crate::sql::ast::BinaryOp::Like => {}
                _ => return None,
            }
            collect_projected_scan_filter_columns(left, fields)?;
            collect_projected_scan_filter_columns(right, fields)
        }
        Expr::IsNull { expr, .. } => collect_projected_scan_filter_columns(expr, fields),
        Expr::InList { expr, values, .. } => {
            collect_projected_scan_filter_columns(expr, fields)?;
            for value in values {
                collect_projected_scan_filter_columns(value, fields)?;
            }
            Some(())
        }
        Expr::Between {
            expr, low, high, ..
        } => {
            collect_projected_scan_filter_columns(expr, fields)?;
            collect_projected_scan_filter_columns(low, fields)?;
            collect_projected_scan_filter_columns(high, fields)
        }
        Expr::Not { expr } => collect_projected_scan_filter_columns(expr, fields),
        Expr::Cast { expr, .. } => collect_projected_scan_filter_columns(expr, fields),
        Expr::Function(_) | Expr::Exists(_) => None,
    }
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
                    ?,
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
                    ?,
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
                    ?,
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
                ?;
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
                        ?,
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
                    ?,
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
                    ?,
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
                    ?,
                ),
                negated: *negated,
            }),
            Expr::Not { expr } => Ok(Expr::Not {
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
                    ?,
                ),
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
                    ?,
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
                ?;
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
}

fn build_logical_plan(
    statement: &crate::sql::ast::ParsedStatement,
) -> Result<LogicalPlan, QueryError> {
    let plan = crate::planner::logical::plan(&crate::sql::binder::BoundStatement {
        statement: statement.clone(),
        indexes: Vec::new(),
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
    outer_row: Option<&'a BatchRow>,
) -> SourceExecution<'a> {
        match source {
            QuerySource::Collection(name) => {
                if let Some(rows) = virtual_views::rows(&env.cassie.catalog, name) {
                    let mut batches = materialize_virtual_rows(rows);
                    if qualify {
                        batches = qualify_batches(batches, name);
                    }
                    ensure_temp_budget(env.controls, &batches)?;
                    return Ok((batches, Vec::new()));
                }

                if let Some(view) = env.cassie.catalog.get_view(name) {
                    let parsed = crate::sql::parser::parse_statement(&view.query)
                        .map_err(|error| QueryError::General(error.0))?;
                    let logical = build_logical_plan(&parsed)?;
                    let mut view_cte_context = CteContext::new();
                    let rows = execute_plan(
                        env.cassie,
                        env.session,
                        &logical,
                        &mut view_cte_context,
                        env.user_functions,
                        env.params,
                        env.controls,
                    )
                    ?;
                    let rows = project_rows_to_schema(rows, &view.schema, name)?;
                    let mut batches = batch::chunk_rows(rows, batch::DEFAULT_BATCH_SIZE);
                    if qualify {
                        batches = qualify_batches(batches, name);
                    }
                    ensure_temp_budget(env.controls, &batches)?;
                    let text_fields = view
                        .schema
                        .fields
                        .iter()
                        .filter(|field| field.data_type == DataType::Text)
                        .map(|field| field.name.clone())
                        .collect::<Vec<_>>();
                    return Ok((batches, text_fields));
                }

                let mut batches = scan::scan(env.cassie, env.session, name)?;
                if qualify {
                    batches = qualify_batches(batches, name);
                }
                ensure_temp_budget(env.controls, &batches)?;
                Ok((batches, env.cassie.catalog.text_fields(name)))
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
            QuerySource::Subquery {
                alias,
                select,
                lateral,
            } => {
                let logical = LogicalPlan {
                    command: None,
                    source: select.source.clone(),
                    collection: alias.clone(),
                    ctes: select.ctes.clone(),
                    distinct: select.distinct,
                    distinct_on: select.distinct_on.clone(),
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
                let rows = execute_plan_with_outer_row(
                    env.cassie,
                    env.session,
                    &logical,
                    &mut subquery_context,
                    env.user_functions,
                    env.params,
                    env.controls,
                    if *lateral { outer_row } else { None },
                )
                ?;
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
                    execute_query_source(env, left, cte_context, true, outer_row)?;
                if source_contains_lateral(right) {
                    let left_rows = batch::flatten_batches(left_batches);
                    let mut joined = Vec::new();

                    for left_row in &left_rows {
                        let (right_batches, _right_text) =
                            execute_query_source(env, right, cte_context, true, Some(left_row))
                                ?;
                        let right_rows = batch::flatten_batches(right_batches);
                        let right_columns = row_columns(&right_rows);
                        let mut matched = false;
                        for right_row in &right_rows {
                            let combined = combine_rows(left_row, right_row);
                            let passes = matches!(kind, JoinKind::Cross)
                                || filter::eval_scalar(
                                    &combined,
                                    on,
                                    env.params,
                                    None,
                                    env.user_functions,
                                    None,
                                    env.session,
                                )?
                                .as_bool();
                            if passes {
                                matched = true;
                                joined.push(combined);
                            }
                        }

                        if !matched && matches!(kind, JoinKind::Left | JoinKind::Full) {
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
                    return Ok((batches, text_fields));
                }

                let (right_batches, _right_text) =
                    execute_query_source(env, right, cte_context, true, outer_row)?;
                let left_rows = batch::flatten_batches(left_batches);
                let right_rows = batch::flatten_batches(right_batches);
                let left_columns = row_columns(&left_rows);
                let right_columns = row_columns(&right_rows);
                let mut joined = Vec::new();
                let mut right_matched = vec![false; right_rows.len()];

                for left_row in &left_rows {
                    let mut matched = false;
                    for (right_index, right_row) in right_rows.iter().enumerate() {
                        let combined = combine_rows(left_row, right_row);
                        let passes = matches!(kind, JoinKind::Cross)
                            || filter::eval_scalar(
                                &combined,
                                on,
                                env.params,
                                None,
                                env.user_functions,
                                None,
                                env.session,
                            )?
                            .as_bool();
                        if passes {
                            matched = true;
                            right_matched[right_index] = true;
                            joined.push(combined);
                        }
                    }

                    if !matched && matches!(kind, JoinKind::Left | JoinKind::Full) {
                        joined.push(combine_row_with_nulls(left_row, &right_columns));
                    }
                }

                if matches!(kind, JoinKind::Right | JoinKind::Full) {
                    for (right_index, right_row) in right_rows.iter().enumerate() {
                        if !right_matched[right_index] {
                            joined.push(combine_nulls_with_row(&left_columns, right_row));
                        }
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

fn combine_batches_with_outer_row(batches: Vec<Batch>, outer_row: &BatchRow) -> Vec<Batch> {
    batches
        .into_iter()
        .map(|batch| {
            batch
                .into_iter()
                .map(|row| combine_rows(outer_row, &row))
                .collect()
        })
        .collect()
}

fn source_contains_lateral(source: &QuerySource) -> bool {
    match source {
        QuerySource::Subquery { lateral, .. } => *lateral,
        QuerySource::Join { left, right, .. } => {
            source_contains_lateral(left) || source_contains_lateral(right)
        }
        QuerySource::Collection(_) | QuerySource::Cte(_) | QuerySource::SingleRow => false,
    }
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

fn combine_nulls_with_row(left_columns: &[String], right: &BatchRow) -> BatchRow {
    let mut values = left_columns
        .iter()
        .map(|column| (column.clone(), Value::Null))
        .collect::<Vec<_>>();
    values.extend(right.entries().iter().cloned());
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

fn project_rows_to_schema(
    rows: Vec<BatchRow>,
    schema: &Schema,
    relation: &str,
) -> Result<Vec<BatchRow>, QueryError> {
    let mut projected = Vec::with_capacity(rows.len());
    for row in rows {
        let entries = row.into_entries();
        if entries.len() < schema.fields.len() {
            return Err(QueryError::General(format!(
                "view '{}' produced {} columns but schema expects {}",
                relation,
                entries.len(),
                schema.fields.len()
            )));
        }

        let mut values = Vec::with_capacity(schema.fields.len());
        for (field, (_name, value)) in schema.fields.iter().zip(entries) {
            values.push((field.name.clone(), value));
        }
        projected.push(BatchRow::new(values));
    }
    Ok(projected)
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

fn apply_window_functions(
    batches: Vec<Batch>,
    projection: &[SelectItem],
    params: &[Value],
    search_context: Option<&filter::SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
) -> Result<Vec<Batch>, QueryError> {
    let windows = projection
        .iter()
        .filter_map(|item| match item {
            SelectItem::WindowFunction { function, alias } => Some((function, alias)),
            _ => None,
        })
        .collect::<Vec<_>>();
    if windows.is_empty() {
        return Ok(batches);
    }

    let mut rows = batch::flatten_batches(batches);
    for (function, alias) in windows {
        let function_name = function.name.to_ascii_lowercase();
        if !matches!(
            function_name.as_str(),
            "row_number" | "rank" | "dense_rank" | "lag" | "lead" | "first_value" | "last_value"
        ) {
            return Err(QueryError::General(format!(
                "unsupported window function '{}'",
                function.name
            )));
        }
        let output_name = alias
            .as_deref()
            .unwrap_or(function.name.as_str())
            .to_string();
        let mut partitions = BTreeMap::<String, Vec<usize>>::new();
        for (index, row) in rows.iter().enumerate() {
            let key = if function.partition_by.is_empty() {
                "__all__".to_string()
            } else {
                function
                    .partition_by
                    .iter()
                    .map(|expr| {
                        filter::evaluate_expr_value(
                            row,
                            expr,
                            params,
                            search_context,
                            user_functions,
                            session,
                            None,
                        )
                        .map(|value| value_sort_key(&value))
                    })
                    .collect::<Result<Vec<_>, _>>()?
                    .join("|")
            };
            partitions.entry(key).or_default().push(index);
        }

        let mut values = vec![Value::Null; rows.len()];
        for indices in partitions.values_mut() {
            indices.sort_by(|left, right| {
                compare_window_rows(
                    &rows[*left],
                    &rows[*right],
                    &function.order_by,
                    params,
                    search_context,
                    user_functions,
                    session,
                )
            });
            let mut dense_rank = 1i64;
            let mut previous_peer_key: Option<String> = None;
            for (position, index) in indices.iter().enumerate() {
                let peer_key = window_peer_key(
                    &rows[*index],
                    &function.order_by,
                    params,
                    search_context,
                    user_functions,
                    session,
                )?;
                if position > 0 && previous_peer_key.as_ref() != Some(&peer_key) {
                    dense_rank += 1;
                }
                previous_peer_key = Some(peer_key);

                values[*index] = match function_name.as_str() {
                    "row_number" => Value::Int64(i64::try_from(position + 1).unwrap_or(i64::MAX)),
                    "rank" => {
                        let peer_position = indices[..=position]
                            .iter()
                            .position(|candidate| {
                                window_peer_key(
                                    &rows[*candidate],
                                    &function.order_by,
                                    params,
                                    search_context,
                                    user_functions,
                                    session,
                                )
                                .ok()
                                    == previous_peer_key
                            })
                            .unwrap_or(position);
                        Value::Int64(i64::try_from(peer_position + 1).unwrap_or(i64::MAX))
                    }
                    "dense_rank" => Value::Int64(dense_rank),
                    "lag" => window_arg_value(
                        indices.get(position.wrapping_sub(1)).copied(),
                        &rows,
                        function,
                        params,
                        search_context,
                        user_functions,
                        session,
                    )?,
                    "lead" => window_arg_value(
                        indices.get(position + 1).copied(),
                        &rows,
                        function,
                        params,
                        search_context,
                        user_functions,
                        session,
                    )?,
                    "first_value" => window_arg_value(
                        indices.first().copied(),
                        &rows,
                        function,
                        params,
                        search_context,
                        user_functions,
                        session,
                    )?,
                    "last_value" => window_arg_value(
                        indices.last().copied(),
                        &rows,
                        function,
                        params,
                        search_context,
                        user_functions,
                        session,
                    )?,
                    _ => Value::Null,
                };
            }
        }

        for (row, value) in rows.iter_mut().zip(values) {
            let mut entries = row.clone().into_entries();
            entries.push((output_name.clone(), value));
            *row = BatchRow::new(entries);
        }
    }

    Ok(batch::chunk_rows(rows, batch::DEFAULT_BATCH_SIZE))
}

fn window_arg_value(
    index: Option<usize>,
    rows: &[BatchRow],
    function: &crate::sql::ast::WindowFunctionCall,
    params: &[Value],
    search_context: Option<&filter::SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
) -> Result<Value, QueryError> {
    let Some(index) = index else {
        return Ok(Value::Null);
    };
    let Some(expr) = function.args.first() else {
        return Ok(Value::Null);
    };
    filter::evaluate_expr_value(
        &rows[index],
        expr,
        params,
        search_context,
        user_functions,
        session,
        None,
    )
}

fn window_peer_key(
    row: &BatchRow,
    order_by: &[crate::sql::ast::OrderExpr],
    params: &[Value],
    search_context: Option<&filter::SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
) -> Result<String, QueryError> {
    if order_by.is_empty() {
        return Ok("__all__".to_string());
    }
    order_by
        .iter()
        .map(|order| {
            filter::evaluate_expr_value(
                row,
                &order.expr,
                params,
                search_context,
                user_functions,
                session,
                None,
            )
            .map(|value| value_sort_key(&value))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|parts| parts.join("|"))
}

fn compare_window_rows(
    left: &BatchRow,
    right: &BatchRow,
    order_by: &[crate::sql::ast::OrderExpr],
    params: &[Value],
    search_context: Option<&filter::SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
) -> CmpOrdering {
    for order in order_by {
        let left_value = filter::evaluate_expr_value(
            left,
            &order.expr,
            params,
            search_context,
            user_functions,
            session,
            None,
        )
        .unwrap_or(Value::Null);
        let right_value = filter::evaluate_expr_value(
            right,
            &order.expr,
            params,
            search_context,
            user_functions,
            session,
            None,
        )
        .unwrap_or(Value::Null);
        let cmp = compare_query_values(&left_value, &right_value);
        if cmp != CmpOrdering::Equal {
            return match order.direction {
                SortDirection::Asc => cmp,
                SortDirection::Desc => cmp.reverse(),
            };
        }
    }
    batch::row_tie_key(left).cmp(&batch::row_tie_key(right))
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
        Expr::Not { expr } => collect_aggregate_specs_from_expr(expr, specs),
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
        Expr::Not { expr } => Expr::Not {
            expr: Box::new(rewrite_aggregate_expr(expr)),
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

fn distinct_on_batches(
    batches: Vec<Batch>,
    distinct_on: &[Expr],
    params: &[Value],
    search_context: Option<&filter::SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
) -> Result<Vec<Batch>, QueryError> {
    let mut seen = HashSet::<String>::new();
    let mut rows = Vec::new();
    for row in batch::flatten_batches(batches) {
        let key = distinct_on
            .iter()
            .map(|expr| {
                filter::evaluate_expr_value(
                    &row,
                    expr,
                    params,
                    search_context,
                    user_functions,
                    session,
                    None,
                )
                .map(|value| value_sort_key(&value))
            })
            .collect::<Result<Vec<_>, _>>()?
            .join("|");
        if seen.insert(key) {
            rows.push(row);
        }
    }
    Ok(batch::chunk_rows(rows, batch::DEFAULT_BATCH_SIZE))
}

fn apply_set_operation(
    left: Vec<BatchRow>,
    right: Vec<BatchRow>,
    set: &SelectSet,
) -> Result<Vec<BatchRow>, QueryError> {
    validate_set_width(&left, &right)?;
    match set.operator {
        SetOperator::UnionAll => {
            let mut rows = left;
            rows.extend(right);
            rows.sort_by_key(row_signature);
            Ok(rows)
        }
        SetOperator::Union => {
            let mut rows = left;
            rows.extend(right);
            let mut unique = BTreeMap::<String, BatchRow>::new();
            for row in rows {
                unique.entry(row_signature(&row)).or_insert(row);
            }
            Ok(unique.into_values().collect())
        }
        SetOperator::Intersect => {
            let right_signatures = right.iter().map(row_signature).collect::<HashSet<_>>();
            let mut unique = BTreeMap::<String, BatchRow>::new();
            for row in left {
                let signature = row_signature(&row);
                if right_signatures.contains(&signature) {
                    unique.entry(signature).or_insert(row);
                }
            }
            Ok(unique.into_values().collect())
        }
        SetOperator::Except => {
            let right_signatures = right.iter().map(row_signature).collect::<HashSet<_>>();
            let mut unique = BTreeMap::<String, BatchRow>::new();
            for row in left {
                let signature = row_signature(&row);
                if !right_signatures.contains(&signature) {
                    unique.entry(signature).or_insert(row);
                }
            }
            Ok(unique.into_values().collect())
        }
    }
}

fn slice_rows(rows: Vec<BatchRow>, offset: Option<i64>, limit: Option<i64>) -> Vec<BatchRow> {
    let offset = offset
        .and_then(|value| usize::try_from(value.max(0)).ok())
        .unwrap_or(0);
    let limit = limit.and_then(|value| usize::try_from(value.max(0)).ok());
    let iter = rows.into_iter().skip(offset);
    match limit {
        Some(limit) => iter.take(limit).collect(),
        None => iter.collect(),
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
        distinct_on: select.distinct_on.clone(),
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
            SelectItem::Wildcard
            | SelectItem::Column { .. }
            | SelectItem::Expr { .. }
            | SelectItem::WindowFunction { .. } => false,
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
        Expr::Not { expr } => format!("not {}", expr_key(expr)),
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

#[allow(clippy::too_many_arguments)]
fn execute_source_query_with_outer_row(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    plan: &LogicalPlan,
    cte_context: &mut CteContext,
    user_functions: &HashMap<String, FunctionMeta>,
    params: &[Value],
    controls: &QueryExecutionControls,
    outer_row: Option<&BatchRow>,
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
        execute_query_source(&env, &plan.source, cte_context, false, outer_row)?;
    if let Some(outer_row) = outer_row {
        batches = combine_batches_with_outer_row(batches, outer_row);
    }
    let candidate_rows = batches.iter().map(|batch| batch.len()).sum::<usize>();

    let fulltext_fields = fulltext_query_fields(plan);
    let uses_hybrid = plan_uses_function(plan, "hybrid_score");
    let uses_vector = plan_uses_vector_operator(plan);
    let search_context = if fulltext_fields.is_empty() {
        None
    } else {
        let (field_boost, field_k1, field_b) = if let QuerySource::Collection(name) = &plan.source {
            let fields = cassie.catalog.text_fields(name);
            let mut boost = HashMap::with_capacity(fields.len());
            for field in fields {
                if let Some(value) = cassie.catalog.get_field_boost(name, &field) {
                    boost.insert(field, value as f64);
                }
            }

            let index_options = load_fulltext_index_options(cassie, name, &fulltext_fields)?;
            for (field, value) in index_options.field_boost {
                boost.insert(field, value);
            }

            (boost, index_options.field_k1, index_options.field_b)
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
            ?,
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

    batches = apply_window_functions(
        batches,
        &plan.projection,
        params,
        search_context.as_ref(),
        user_functions,
        session,
    )?;
    ensure_temp_budget(controls, &batches)?;

    if !plan.distinct_on.is_empty() {
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
        batches = distinct_on_batches(
            batches,
            &plan.distinct_on,
            params,
            search_context.as_ref(),
            user_functions,
            session,
        )?;
        ensure_temp_budget(controls, &batches)?;
    } else if plan.set.is_none() && !plan.order.is_empty() {
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

    let mut rows = batch::flatten_batches(batches);
    if let Some(set) = &plan.set {
        let right_plan = logical_plan_from_select(&set.right);
        let right_rows = execute_plan(
            cassie,
            session,
            &right_plan,
            cte_context,
            user_functions,
            params,
            controls,
        )?;
        rows = apply_set_operation(rows, right_rows, set)?;
    }
    if plan.set.is_some() && !plan.order.is_empty() {
        rows = sort::sort_rows(
            rows,
            &plan.order,
            &plan.projection,
            params,
            search_context.as_ref(),
            user_functions,
            session,
        )?;
    }
    rows = slice_rows(rows, plan.offset, plan.limit);

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

fn load_fulltext_index_options(
    cassie: &Cassie,
    collection: &str,
    requested_fields: &HashSet<String>,
) -> Result<FulltextIndexOptions, QueryError> {
    let cache_key = FulltextIndexOptionsCacheKey::new(
        cassie.runtime.schema_epoch(),
        collection,
        requested_fields.iter().cloned(),
    );
    if let Some(options) = cassie.runtime.fulltext_index_options_lookup(&cache_key) {
        return Ok(options);
    }

    let mut field_boost = HashMap::new();
    let mut field_k1 = HashMap::new();
    let mut field_b = HashMap::new();
    let mut seen_fields = HashSet::new();

    for index in cassie.catalog.list_indexes(collection) {
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

    let options = FulltextIndexOptions {
        field_boost,
        field_k1,
        field_b,
    };
    cassie
        .runtime
        .store_fulltext_index_options(cache_key, options.clone());
    Ok(options)
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

pub(crate) fn plan_needs_user_functions(plan: &LogicalPlan) -> bool {
    query_source_needs_user_functions(&plan.source)
        || plan.projection.iter().any(select_item_needs_user_functions)
        || plan.filter.as_ref().is_some_and(expr_needs_user_functions)
        || plan.distinct_on.iter().any(expr_needs_user_functions)
        || plan.group_by.iter().any(expr_needs_user_functions)
        || plan.having.as_ref().is_some_and(expr_needs_user_functions)
        || plan
            .order
            .iter()
            .any(|order| expr_needs_user_functions(&order.expr))
        || plan.ctes.iter().any(cte_needs_user_functions)
        || plan
            .set
            .as_ref()
            .is_some_and(|set| select_needs_user_functions(&set.right))
}

fn cte_needs_user_functions(cte: &CommonTableExpression) -> bool {
    match &cte.query {
        CteQuery::Simple(statement) => parsed_statement_needs_user_functions(statement),
        CteQuery::Recursive { base, recursive } => {
            parsed_statement_needs_user_functions(base)
                || parsed_statement_needs_user_functions(recursive)
        }
    }
}

fn parsed_statement_needs_user_functions(statement: &crate::sql::ast::ParsedStatement) -> bool {
    match &statement.statement {
        QueryStatement::Select(select) => select_needs_user_functions(select),
        _ => false,
    }
}

fn select_needs_user_functions(select: &SelectStatement) -> bool {
    query_source_needs_user_functions(&select.source)
        || select
            .projection
            .iter()
            .any(select_item_needs_user_functions)
        || select
            .filter
            .as_ref()
            .is_some_and(expr_needs_user_functions)
        || select.distinct_on.iter().any(expr_needs_user_functions)
        || select.group_by.iter().any(expr_needs_user_functions)
        || select
            .having
            .as_ref()
            .is_some_and(expr_needs_user_functions)
        || select
            .order
            .iter()
            .any(|order| expr_needs_user_functions(&order.expr))
        || select.ctes.iter().any(cte_needs_user_functions)
        || select
            .set
            .as_ref()
            .is_some_and(|set| select_needs_user_functions(&set.right))
}

fn query_source_needs_user_functions(source: &QuerySource) -> bool {
    match source {
        QuerySource::Collection(_) | QuerySource::Cte(_) | QuerySource::SingleRow => false,
        QuerySource::Subquery { select, .. } => select_needs_user_functions(select),
        QuerySource::Join {
            left, right, on, ..
        } => {
            query_source_needs_user_functions(left)
                || query_source_needs_user_functions(right)
                || expr_needs_user_functions(on)
        }
    }
}

fn select_item_needs_user_functions(item: &SelectItem) -> bool {
    match item {
        SelectItem::Function { function, .. } => function_needs_user_functions(function),
        SelectItem::Expr { expr, .. } => expr_needs_user_functions(expr),
        SelectItem::Column { .. } | SelectItem::Wildcard | SelectItem::WindowFunction { .. } => {
            false
        }
    }
}

fn expr_needs_user_functions(expr: &Expr) -> bool {
    match expr {
        Expr::Binary { left, right, .. } => {
            expr_needs_user_functions(left) || expr_needs_user_functions(right)
        }
        Expr::IsNull { expr, .. } | Expr::Cast { expr, .. } => expr_needs_user_functions(expr),
        Expr::InList { expr, values, .. } => {
            expr_needs_user_functions(expr) || values.iter().any(expr_needs_user_functions)
        }
        Expr::Between {
            expr, low, high, ..
        } => {
            expr_needs_user_functions(expr)
                || expr_needs_user_functions(low)
                || expr_needs_user_functions(high)
        }
        Expr::Not { expr } => expr_needs_user_functions(expr),
        Expr::Exists(statement) => parsed_statement_needs_user_functions(statement),
        Expr::Function(function) => function_needs_user_functions(function),
        Expr::Column(_)
        | Expr::Param(_)
        | Expr::StringLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::BoolLiteral(_)
        | Expr::Null => false,
    }
}

fn function_needs_user_functions(function: &FunctionCall) -> bool {
    let name = function.name.to_ascii_lowercase();
    let is_builtin = matches!(
        name.as_str(),
        "search"
            | "search_score"
            | "vector_distance"
            | "vector_score"
            | "cosine_distance"
            | "dot_product"
            | "hybrid_score"
            | "snippet"
            | "version"
            | "current_schema"
            | "current_database"
            | "current_user"
            | "session_user"
            | "current_role"
            | "length"
            | "len"
            | "lower"
            | "upper"
            | "substring"
            | "trim"
            | "concat"
            | "coalesce"
            | "abs"
            | "cast"
    ) || crate::sql::functions::is_aggregate_function(&function.name);

    !is_builtin || function.args.iter().any(expr_needs_user_functions)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn plan_for_sql(sql: &str) -> LogicalPlan {
        let parsed = crate::sql::parse_statement(sql).expect("parse statement");
        build_logical_plan(&parsed).expect("build logical plan")
    }

    #[test]
    fn should_detect_unordered_fulltext_fast_path_for_matching_search_query() {
        // Arrange
        let plan = plan_for_sql(
            "SELECT id, search_score(body, 'alpha') AS score FROM bench_documents WHERE search(body, 'alpha')",
        );

        // Act
        let spec = fulltext_filtered_read_spec(&plan);

        // Assert
        let spec = spec.expect("unordered fulltext fast path");
        assert_eq!(spec.collection, "bench_documents");
        assert_eq!(spec.text_field, "body");
        assert_eq!(spec.query, "alpha");
        assert_eq!(spec.score_column, "score");
        assert_eq!(spec.columns.len(), 1);
        assert_eq!(spec.columns[0].name, "id");
        assert_eq!(spec.columns[0].output_name, "id");
    }

    #[test]
    fn should_reject_unordered_fulltext_fast_path_for_mismatched_search_query() {
        // Arrange
        let plan = plan_for_sql(
            "SELECT id, search_score(body, 'alpha') AS score FROM bench_documents WHERE search(body, 'bravo')",
        );

        // Act
        let spec = fulltext_filtered_read_spec(&plan);

        // Assert
        assert!(spec.is_none());
    }

    #[test]
    fn should_reject_unordered_fulltext_fast_path_for_additional_filters() {
        // Arrange
        let plan = plan_for_sql(
            "SELECT id, search_score(body, 'alpha') AS score FROM bench_documents WHERE search(body, 'alpha') AND status = 'approved'",
        );

        // Act
        let spec = fulltext_filtered_read_spec(&plan);

        // Assert
        assert!(spec.is_none());
    }

    #[test]
    fn should_reject_unordered_fulltext_fast_path_for_wildcard_projection() {
        // Arrange
        let plan = plan_for_sql("SELECT * FROM bench_documents WHERE search(body, 'alpha')");

        // Act
        let spec = fulltext_filtered_read_spec(&plan);

        // Assert
        assert!(spec.is_none());
    }

    #[test]
    fn should_build_projected_read_spec_without_filter() {
        // Arrange
        let plan = plan_for_sql("SELECT id, title FROM bench_documents LIMIT 20");

        // Act
        let spec = projected_filtered_read_spec(&plan);

        // Assert
        let spec = spec.expect("projected read spec");
        assert_eq!(spec.collection, "bench_documents");
        assert_eq!(spec.scan_fields, vec!["title".to_string()]);
    }

    #[test]
    fn should_push_limit_into_projected_read_spec_without_filter() {
        // Arrange
        let plan = plan_for_sql("SELECT id, title FROM bench_documents LIMIT 20");

        // Act
        let spec = projected_filtered_read_spec(&plan);

        // Assert
        let spec = spec.expect("projected read spec");
        assert_eq!(spec.scan_limit, Some(20));
    }

    #[test]
    fn should_include_offset_in_projected_read_spec_scan_limit() {
        // Arrange
        let plan = plan_for_sql("SELECT id, title FROM bench_documents LIMIT 20 OFFSET 5");

        // Act
        let spec = projected_filtered_read_spec(&plan);

        // Assert
        let spec = spec.expect("projected read spec");
        assert_eq!(spec.scan_limit, Some(25));
    }

    #[test]
    fn should_not_push_limit_into_projected_read_spec_when_filter_is_present() {
        // Arrange
        let plan = plan_for_sql(
            "SELECT id, title FROM bench_documents WHERE status = 'approved' LIMIT 20",
        );

        // Act
        let spec = projected_filtered_read_spec(&plan);

        // Assert
        let spec = spec.expect("projected read spec");
        assert_eq!(spec.scan_limit, None);
    }

    #[test]
    fn should_detect_projected_scan_pushdown_for_literal_equality() {
        // Arrange
        let plan = plan_for_sql("SELECT id, title FROM bench_documents WHERE title = 'alpha'");
        let filter = plan.filter.as_ref().expect("filter");

        // Act
        let pushdown = projected_scan_pushdown_filter(filter);

        // Assert
        let pushdown = pushdown.expect("pushdown filter");
        assert_eq!(pushdown.field, "title");
        assert_eq!(pushdown.value, Value::String("alpha".to_string()));
    }

    #[test]
    fn should_reject_projected_scan_pushdown_for_row_id_equality() {
        // Arrange
        let plan = plan_for_sql("SELECT id, title FROM bench_documents WHERE id = 'doc-1'");
        let filter = plan.filter.as_ref().expect("filter");

        // Act
        let pushdown = projected_scan_pushdown_filter(filter);

        // Assert
        assert!(pushdown.is_none());
    }

    #[test]
    fn should_skip_user_function_catalog_for_builtin_only_plan() {
        // Arrange
        let plan =
            plan_for_sql("SELECT id FROM bench_documents WHERE score >= 10 ORDER BY id LIMIT 20");

        // Act
        let needs_user_functions = plan_needs_user_functions(&plan);

        // Assert
        assert!(!needs_user_functions);
    }

    #[test]
    fn should_require_user_function_catalog_for_user_defined_function_plan() {
        // Arrange
        let plan =
            plan_for_sql("SELECT my_udf(title) AS normalized_title FROM bench_documents LIMIT 20");

        // Act
        let needs_user_functions = plan_needs_user_functions(&plan);

        // Assert
        assert!(needs_user_functions);
    }

    #[test]
    fn should_report_execution_breakdown_for_projected_filtered_read() {
        // Arrange
        std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
        let mut path = std::env::temp_dir();
        path.push(format!(
            "cassie-execution-breakdown-{}",
            uuid::Uuid::new_v4()
        ));
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        let collection = "breakdown_documents";
        let schema = Schema {
            fields: vec![FieldSchema {
                name: "title".to_string(),
                data_type: DataType::Text,
                nullable: true,
            }],
        };
        cassie
            .midge
            .create_collection(collection, schema.clone())
            .expect("create collection");
        cassie.register_collection(collection, schema);
        cassie
            .midge
            .put_document(
                collection,
                Some("doc-1".to_string()),
                serde_json::json!({"title": "alpha"}),
            )
            .expect("put document");

        let logical =
            plan_for_sql("SELECT id, title FROM breakdown_documents WHERE title = 'alpha'");
        let physical = crate::planner::physical::build(logical);

        // Act
        let output = run_with_execution_breakdown(&cassie, physical, vec![])
            
            .expect("execution breakdown");

        // Assert
        assert_eq!(output.result.rows.len(), 1);
        assert_eq!(
            output.result.rows[0],
            vec![
                Value::String("doc-1".to_string()),
                Value::String("alpha".to_string()),
            ]
        );
        assert!(output.breakdown.scan_us > 0 || output.breakdown.row_decode_us > 0);
        assert_eq!(output.breakdown.filter_us, 0);
        assert!(output.breakdown.projection_us > 0);
        assert!(output.breakdown.result_build_us > 0);

        let _ = std::fs::remove_dir_all(path);
    }
}
