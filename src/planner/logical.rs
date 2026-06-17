use crate::app::CassieError;
use crate::sql::{
    ast::{
        AlterTableOperation, AlterTableStatement, CommonTableExpression, CreateFunctionStatement,
        CreateIndexStatement, CreateProcedureStatement, CreateSchemaStatement,
        CreateTableStatement, DropFunctionStatement, DropIndexStatement, DropProcedureStatement,
        DropTableStatement, Expr, OrderExpr, QuerySource, QueryStatement, SelectItem,
        SelectStatement,
    },
    binder::BoundStatement,
};

#[derive(Debug, Clone)]
pub struct LogicalPlan {
    pub command: Option<LogicalCommand>,
    pub source: QuerySource,
    pub collection: String,
    pub ctes: Vec<CommonTableExpression>,
    pub projection: Vec<SelectItem>,
    pub filter: Option<Expr>,
    pub order: Vec<OrderExpr>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Clone)]
pub enum LogicalCommand {
    CreateTable(CreateTableStatement),
    DropTable(DropTableStatement),
    AlterTable(AlterTableStatement),
    CreateFunction(CreateFunctionStatement),
    DropFunction(DropFunctionStatement),
    CreateProcedure(CreateProcedureStatement),
    DropProcedure(DropProcedureStatement),
    CreateSchema(CreateSchemaStatement),
    CreateIndex(CreateIndexStatement),
    DropIndex(DropIndexStatement),
    CallProcedure(crate::sql::ast::CallProcedureStatement),
}

pub fn plan(bound: &BoundStatement) -> Result<LogicalPlan, CassieError> {
    match &bound.statement.statement {
        QueryStatement::Select(select) => {
            validate_logical_plan(select)?;
            Ok(LogicalPlan {
                command: None,
                source: select.source.clone(),
                collection: source_name(&select.source),
                ctes: select.ctes.clone(),
                projection: select.projection.clone(),
                filter: select.filter.clone(),
                order: select.order.clone(),
                limit: select.limit,
                offset: select.offset,
            })
        }
        QueryStatement::Insert(statement) => Err(CassieError::Planner(format!(
            "INSERT statement is not supported: {}",
            statement.table
        ))),
        QueryStatement::Update(statement) => Err(CassieError::Planner(format!(
            "UPDATE statement is not supported: {}",
            statement.table
        ))),
        QueryStatement::Delete(statement) => Err(CassieError::Planner(format!(
            "DELETE statement is not supported: {}",
            statement.table
        ))),
        QueryStatement::CreateTable(statement) => {
            if statement.table.trim().is_empty() {
                return Err(CassieError::Planner(
                    "CREATE TABLE requires a table name".into(),
                ));
            }
            if statement.fields.is_empty() {
                return Err(CassieError::Planner(
                    "CREATE TABLE requires at least one column".into(),
                ));
            }

            Ok(LogicalPlan {
                command: Some(LogicalCommand::CreateTable(statement.clone())),
                source: QuerySource::Collection(statement.table.clone()),
                collection: statement.table.clone(),
                ctes: Vec::new(),
                projection: Vec::new(),
                filter: None,
                order: Vec::new(),
                limit: None,
                offset: Some(0),
            })
        }
        QueryStatement::DropTable(statement) => {
            if statement.table.trim().is_empty() {
                return Err(CassieError::Planner(
                    "DROP TABLE requires a table name".into(),
                ));
            }

            Ok(LogicalPlan {
                command: Some(LogicalCommand::DropTable(statement.clone())),
                source: QuerySource::Collection(statement.table.clone()),
                collection: statement.table.clone(),
                ctes: Vec::new(),
                projection: Vec::new(),
                filter: None,
                order: Vec::new(),
                limit: None,
                offset: Some(0),
            })
        }
        QueryStatement::AlterTable(statement) => {
            if statement.table.trim().is_empty() {
                return Err(CassieError::Planner(
                    "ALTER TABLE requires a table name".into(),
                ));
            }

            validate_alter_command(statement)?;

            Ok(LogicalPlan {
                command: Some(LogicalCommand::AlterTable(statement.clone())),
                source: QuerySource::Collection(statement.table.clone()),
                collection: statement.table.clone(),
                ctes: Vec::new(),
                projection: Vec::new(),
                filter: None,
                order: Vec::new(),
                limit: None,
                offset: Some(0),
            })
        }
        QueryStatement::CreateSchema(statement) => {
            if statement.schema.trim().is_empty() {
                return Err(CassieError::Planner("CREATE SCHEMA requires a name".into()));
            }

            Ok(LogicalPlan {
                command: Some(LogicalCommand::CreateSchema(statement.clone())),
                source: QuerySource::Collection(statement.schema.clone()),
                collection: statement.schema.clone(),
                ctes: Vec::new(),
                projection: Vec::new(),
                filter: None,
                order: Vec::new(),
                limit: None,
                offset: Some(0),
            })
        }
        QueryStatement::CreateFunction(statement) => {
            if statement.name.trim().is_empty() {
                return Err(CassieError::Planner(
                    "CREATE FUNCTION requires a name".into(),
                ));
            }

            Ok(LogicalPlan {
                command: Some(LogicalCommand::CreateFunction(statement.clone())),
                source: QuerySource::Collection(statement.name.clone()),
                collection: statement.name.clone(),
                ctes: Vec::new(),
                projection: Vec::new(),
                filter: None,
                order: Vec::new(),
                limit: None,
                offset: Some(0),
            })
        }
        QueryStatement::DropFunction(statement) => {
            if statement.name.trim().is_empty() {
                return Err(CassieError::Planner("DROP FUNCTION requires a name".into()));
            }

            Ok(LogicalPlan {
                command: Some(LogicalCommand::DropFunction(statement.clone())),
                source: QuerySource::Collection(statement.name.clone()),
                collection: statement.name.clone(),
                ctes: Vec::new(),
                projection: Vec::new(),
                filter: None,
                order: Vec::new(),
                limit: None,
                offset: Some(0),
            })
        }
        QueryStatement::CreateProcedure(statement) => {
            if statement.name.trim().is_empty() {
                return Err(CassieError::Planner(
                    "CREATE PROCEDURE requires a name".into(),
                ));
            }

            Ok(LogicalPlan {
                command: Some(LogicalCommand::CreateProcedure(statement.clone())),
                source: QuerySource::Collection(statement.name.clone()),
                collection: statement.name.clone(),
                ctes: Vec::new(),
                projection: Vec::new(),
                filter: None,
                order: Vec::new(),
                limit: None,
                offset: Some(0),
            })
        }
        QueryStatement::DropProcedure(statement) => {
            if statement.name.trim().is_empty() {
                return Err(CassieError::Planner(
                    "DROP PROCEDURE requires a name".into(),
                ));
            }

            Ok(LogicalPlan {
                command: Some(LogicalCommand::DropProcedure(statement.clone())),
                source: QuerySource::Collection(statement.name.clone()),
                collection: statement.name.clone(),
                ctes: Vec::new(),
                projection: Vec::new(),
                filter: None,
                order: Vec::new(),
                limit: None,
                offset: Some(0),
            })
        }
        QueryStatement::CallProcedure(statement) => {
            if statement.name.trim().is_empty() {
                return Err(CassieError::Planner(
                    "CALL requires a procedure name".into(),
                ));
            }

            Ok(LogicalPlan {
                command: Some(LogicalCommand::CallProcedure(statement.clone())),
                source: QuerySource::Collection(statement.name.clone()),
                collection: statement.name.clone(),
                ctes: Vec::new(),
                projection: Vec::new(),
                filter: None,
                order: Vec::new(),
                limit: None,
                offset: Some(0),
            })
        }
        QueryStatement::CreateIndex(statement) => {
            if statement.table.trim().is_empty() {
                return Err(CassieError::Planner(
                    "CREATE INDEX requires a collection name".into(),
                ));
            }
            if statement.name.trim().is_empty() {
                return Err(CassieError::Planner(
                    "CREATE INDEX requires an index name".into(),
                ));
            }
            if statement.field.trim().is_empty() {
                return Err(CassieError::Planner(
                    "CREATE INDEX requires an indexed field".into(),
                ));
            }

            Ok(LogicalPlan {
                command: Some(LogicalCommand::CreateIndex(statement.clone())),
                source: QuerySource::Collection(statement.table.clone()),
                collection: statement.table.clone(),
                ctes: Vec::new(),
                projection: Vec::new(),
                filter: None,
                order: Vec::new(),
                limit: None,
                offset: Some(0),
            })
        }
        QueryStatement::DropIndex(statement) => {
            if statement.table.trim().is_empty() {
                return Err(CassieError::Planner(
                    "DROP INDEX requires a collection name".into(),
                ));
            }
            if statement.name.trim().is_empty() {
                return Err(CassieError::Planner(
                    "DROP INDEX requires an index name".into(),
                ));
            }

            Ok(LogicalPlan {
                command: Some(LogicalCommand::DropIndex(statement.clone())),
                source: QuerySource::Collection(statement.table.clone()),
                collection: statement.table.clone(),
                ctes: Vec::new(),
                projection: Vec::new(),
                filter: None,
                order: Vec::new(),
                limit: None,
                offset: Some(0),
            })
        }
    }
}

fn source_name(source: &QuerySource) -> String {
    match source {
        QuerySource::Collection(name) | QuerySource::Cte(name) => name.clone(),
    }
}

fn validate_alter_command(statement: &AlterTableStatement) -> Result<(), CassieError> {
    if statement.table.trim().is_empty() {
        return Err(CassieError::Planner(
            "ALTER TABLE requires a table name".into(),
        ));
    }
    match &statement.operation {
        AlterTableOperation::AddColumn { field, .. } => {
            if field.trim().is_empty() {
                return Err(CassieError::Planner(
                    "ALTER TABLE ADD COLUMN requires a field".into(),
                ));
            }
        }
        AlterTableOperation::DropColumn { field } => {
            if field.trim().is_empty() {
                return Err(CassieError::Planner(
                    "ALTER TABLE DROP COLUMN requires a field".into(),
                ));
            }
            if field.trim().eq_ignore_ascii_case("id") {
                return Err(CassieError::Planner(
                    "ALTER TABLE cannot drop reserved field 'id'".into(),
                ));
            }
        }
        AlterTableOperation::RenameTo { table } => {
            if table.trim().is_empty() {
                return Err(CassieError::Planner(
                    "ALTER TABLE RENAME TO requires a table name".into(),
                ));
            }
        }
    }
    Ok(())
}

fn validate_logical_plan(select: &SelectStatement) -> Result<(), CassieError> {
    if source_name(&select.source).trim().is_empty() {
        return Err(CassieError::Planner(
            "planner cannot build plan for empty source name".to_string(),
        ));
    }

    if select.projection.is_empty() {
        return Err(CassieError::Planner(
            "planner cannot build plan with empty projection".to_string(),
        ));
    }

    if let Some(limit) = select.limit {
        if limit < 0 {
            return Err(CassieError::Planner(format!(
                "planner cannot build plan with negative limit: {limit}"
            )));
        }
    }

    if let Some(offset) = select.offset {
        if offset < 0 {
            return Err(CassieError::Planner(format!(
                "planner cannot build plan with negative offset: {offset}"
            )));
        }
    }

    Ok(())
}
