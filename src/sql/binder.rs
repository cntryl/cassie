use std::collections::{HashMap, HashSet};
use std::mem;

use crate::app::CassieError;
use crate::catalog::{is_reserved_namespace, virtual_views, Catalog, CollectionSchema, IndexMeta};
use crate::embeddings::DistanceMetric;
use crate::search::bm25;
use crate::sql::ast::{
    AlterSchemaOperation, AlterSchemaStatement, AlterTableOperation, AlterTableStatement,
    CallProcedureStatement, CommonTableExpression, CreateFunctionStatement, CreateIndexStatement,
    CreateProcedureStatement, CreateSchemaStatement, CreateViewStatement, CteQuery,
    DropFunctionStatement, DropIndexStatement, DropProcedureStatement, DropSchemaStatement,
    DropViewStatement, Expr, FunctionCall, InsertSource, OrderExpr, ParsedStatement, QuerySource,
    QueryStatement, SelectItem, SelectSet, SelectStatement,
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
#[path = "binder/select.rs"]
mod select;
#[path = "binder/validation.rs"]
mod validation;

use commands::*;
pub use inference::infer_select_schema;
use routines::*;
use schema::*;
use select::*;
use validation::*;

#[derive(Debug, Clone)]
pub struct BoundStatement {
    pub statement: ParsedStatement,
    pub indexes: Vec<IndexMeta>,
}

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
        QuerySource::Cte(_) | QuerySource::SingleRow => None,
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
            let select = bind_select(select, catalog, outer_scope)?;
            Ok(ParsedStatement {
                raw_sql,
                statement: QueryStatement::Select(select),
            })
        }
        QueryStatement::Explain(statement) => {
            let inner = bind_statement(*statement.statement, catalog, outer_scope)?;
            Ok(ParsedStatement {
                raw_sql,
                statement: QueryStatement::Explain(crate::sql::ast::ExplainStatement {
                    analyze: statement.analyze,
                    statement: Box::new(inner),
                }),
            })
        }
        QueryStatement::Show(statement) => {
            let mut clone = statement.clone();
            clone.variable = clone.variable.trim().to_string();
            Ok(ParsedStatement {
                raw_sql,
                statement: QueryStatement::Show(clone),
            })
        }
        QueryStatement::Set(statement) => {
            let mut clone = statement.clone();
            clone.variable = clone.variable.trim().to_string();
            clone.value = clone.value.map(|value| value.trim().to_string());
            Ok(ParsedStatement {
                raw_sql,
                statement: QueryStatement::Set(clone),
            })
        }
        QueryStatement::CreateTable(statement) => {
            let statement = bind_create_table(statement, catalog)?;
            Ok(ParsedStatement {
                raw_sql,
                statement: QueryStatement::CreateTable(statement),
            })
        }
        QueryStatement::DropTable(statement) => {
            let statement = bind_drop_table(statement, catalog)?;
            Ok(ParsedStatement {
                raw_sql,
                statement: QueryStatement::DropTable(statement),
            })
        }
        QueryStatement::AlterTable(statement) => {
            let statement = bind_alter_table(statement, catalog)?;
            Ok(ParsedStatement {
                raw_sql,
                statement: QueryStatement::AlterTable(statement),
            })
        }
        QueryStatement::CreateIndex(statement) => {
            let statement = bind_create_index(statement, catalog)?;
            Ok(ParsedStatement {
                raw_sql,
                statement: QueryStatement::CreateIndex(statement),
            })
        }
        QueryStatement::DropIndex(statement) => {
            let statement = bind_drop_index(statement, catalog)?;
            Ok(ParsedStatement {
                raw_sql,
                statement: QueryStatement::DropIndex(statement),
            })
        }
        QueryStatement::CreateRollup(statement) => {
            let statement = bind_create_rollup(statement, catalog)?;
            Ok(ParsedStatement {
                raw_sql,
                statement: QueryStatement::CreateRollup(statement),
            })
        }
        QueryStatement::RefreshRollup(statement) => {
            let name = statement.name.trim().to_string();
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
            Ok(ParsedStatement {
                raw_sql,
                statement: QueryStatement::RefreshRollup(crate::sql::ast::RefreshRollupStatement {
                    name,
                }),
            })
        }
        QueryStatement::DropRollup(statement) => {
            let name = statement.name.trim().to_string();
            if name.is_empty() {
                return Err(CassieError::Planner("DROP ROLLUP requires a name".into()));
            }
            if !statement.if_exists && catalog.get_rollup(&name).is_none() {
                return Err(CassieError::Planner(format!(
                    "rollup '{name}' does not exist"
                )));
            }
            Ok(ParsedStatement {
                raw_sql,
                statement: QueryStatement::DropRollup(crate::sql::ast::DropRollupStatement {
                    name,
                    if_exists: statement.if_exists,
                }),
            })
        }
        QueryStatement::CreateSchema(statement) => {
            let schema = statement.schema.trim().to_string();
            if schema.is_empty() {
                return Err(CassieError::Planner("CREATE SCHEMA requires a name".into()));
            }

            if is_reserved_namespace(&schema) {
                return Err(CassieError::Unsupported(format!(
                    "namespace '{schema}' is reserved"
                )));
            }
            if !statement.if_not_exists && catalog.namespace_exists(&schema) {
                return Err(CassieError::Planner(format!(
                    "namespace '{schema}' already exists"
                )));
            }

            Ok(ParsedStatement {
                raw_sql,
                statement: QueryStatement::CreateSchema(CreateSchemaStatement {
                    schema,
                    if_not_exists: statement.if_not_exists,
                }),
            })
        }
        QueryStatement::DropSchema(statement) => {
            let statement = bind_drop_schema(statement, catalog)?;
            Ok(ParsedStatement {
                raw_sql,
                statement: QueryStatement::DropSchema(statement),
            })
        }
        QueryStatement::AlterSchema(statement) => {
            let statement = bind_alter_schema(statement, catalog)?;
            Ok(ParsedStatement {
                raw_sql,
                statement: QueryStatement::AlterSchema(statement),
            })
        }
        QueryStatement::CreateView(statement) => {
            let statement = bind_create_view(statement, catalog)?;
            Ok(ParsedStatement {
                raw_sql,
                statement: QueryStatement::CreateView(statement),
            })
        }
        QueryStatement::DropView(statement) => {
            let statement = bind_drop_view(statement, catalog)?;
            Ok(ParsedStatement {
                raw_sql,
                statement: QueryStatement::DropView(statement),
            })
        }
        QueryStatement::CreateRole(statement) => Ok(ParsedStatement {
            raw_sql,
            statement: QueryStatement::CreateRole(statement),
        }),
        QueryStatement::AlterRole(statement) => Ok(ParsedStatement {
            raw_sql,
            statement: QueryStatement::AlterRole(statement),
        }),
        QueryStatement::DropRole(statement) => Ok(ParsedStatement {
            raw_sql,
            statement: QueryStatement::DropRole(statement),
        }),
        QueryStatement::CreateFunction(statement) => {
            let statement = bind_create_function(statement, catalog)?;
            Ok(ParsedStatement {
                raw_sql,
                statement: QueryStatement::CreateFunction(statement),
            })
        }
        QueryStatement::DropFunction(statement) => {
            let statement = bind_drop_function(statement, catalog)?;
            Ok(ParsedStatement {
                raw_sql,
                statement: QueryStatement::DropFunction(statement),
            })
        }
        QueryStatement::CreateProcedure(statement) => {
            let statement = bind_create_procedure(statement, catalog)?;
            Ok(ParsedStatement {
                raw_sql,
                statement: QueryStatement::CreateProcedure(statement),
            })
        }
        QueryStatement::DropProcedure(statement) => {
            let statement = bind_drop_procedure(statement, catalog)?;
            Ok(ParsedStatement {
                raw_sql,
                statement: QueryStatement::DropProcedure(statement),
            })
        }
        QueryStatement::CallProcedure(statement) => {
            let statement = bind_call_procedure(statement, catalog)?;
            Ok(ParsedStatement {
                raw_sql,
                statement: QueryStatement::CallProcedure(statement),
            })
        }
        QueryStatement::Insert(statement) => {
            let statement = bind_insert(statement, catalog)?;
            Ok(ParsedStatement {
                raw_sql,
                statement: QueryStatement::Insert(statement),
            })
        }
        QueryStatement::Update(statement) => {
            let statement = bind_update(statement, catalog)?;
            Ok(ParsedStatement {
                raw_sql,
                statement: QueryStatement::Update(statement),
            })
        }
        QueryStatement::Delete(statement) => {
            let statement = bind_delete(statement, catalog)?;
            Ok(ParsedStatement {
                raw_sql,
                statement: QueryStatement::Delete(statement),
            })
        }
        QueryStatement::Transaction(statement) => Ok(ParsedStatement {
            raw_sql,
            statement: QueryStatement::Transaction(statement),
        }),
    }
}
