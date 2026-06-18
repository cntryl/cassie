use crate::app::CassieError;
use crate::sql::{
    ast::{
        AlterRoleStatement, AlterTableOperation, AlterTableStatement, CommonTableExpression,
        CreateFunctionStatement, CreateIndexStatement, CreateProcedureStatement,
        CreateRoleStatement, CreateSchemaStatement, CreateTableStatement, CreateViewStatement,
        DeleteStatement, DropFunctionStatement, DropIndexStatement, DropProcedureStatement,
        DropRoleStatement, DropTableStatement, DropViewStatement, Expr, InsertStatement,
        OrderExpr, QuerySource, QueryStatement, SelectItem, SelectStatement, SetStatement,
        ShowStatement, UpdateStatement,
    },
    binder::BoundStatement,
};

#[derive(Debug, Clone)]
pub struct LogicalPlan {
    pub command: Option<LogicalCommand>,
    pub source: QuerySource,
    pub collection: String,
    pub ctes: Vec<CommonTableExpression>,
    pub distinct: bool,
    pub projection: Vec<SelectItem>,
    pub filter: Option<Expr>,
    pub group_by: Vec<Expr>,
    pub having: Option<Expr>,
    pub order: Vec<OrderExpr>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub set: Option<Box<crate::sql::ast::SelectSet>>,
}

#[derive(Debug, Clone)]
pub enum LogicalCommand {
    CreateTable(CreateTableStatement),
    DropTable(DropTableStatement),
    AlterTable(AlterTableStatement),
    CreateRole(CreateRoleStatement),
    AlterRole(AlterRoleStatement),
    DropRole(DropRoleStatement),
    CreateFunction(CreateFunctionStatement),
    DropFunction(DropFunctionStatement),
    CreateProcedure(CreateProcedureStatement),
    DropProcedure(DropProcedureStatement),
    CreateSchema(CreateSchemaStatement),
    CreateView(CreateViewStatement),
    DropView(DropViewStatement),
    CreateIndex(CreateIndexStatement),
    DropIndex(DropIndexStatement),
    CallProcedure(crate::sql::ast::CallProcedureStatement),
    Show(ShowStatement),
    Set(SetStatement),
    Insert(InsertStatement),
    Update(UpdateStatement),
    Delete(DeleteStatement),
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
                distinct: select.distinct,
                projection: select.projection.clone(),
                filter: select.filter.clone(),
                group_by: select.group_by.clone(),
                having: select.having.clone(),
                order: select.order.clone(),
                limit: select.limit,
                offset: select.offset,
                set: select.set.clone(),
            })
        }
        QueryStatement::Show(statement) => Ok(LogicalPlan {
            command: Some(LogicalCommand::Show(statement.clone())),
            source: QuerySource::SingleRow,
            collection: String::new(),
            ctes: Vec::new(),
            distinct: false,
            projection: Vec::new(),
            filter: None,
            group_by: Vec::new(),
            having: None,
            order: Vec::new(),
            limit: None,
            offset: None,
            set: None,
        }),
        QueryStatement::Set(statement) => Ok(LogicalPlan {
            command: Some(LogicalCommand::Set(statement.clone())),
            source: QuerySource::SingleRow,
            collection: String::new(),
            ctes: Vec::new(),
            distinct: false,
            projection: Vec::new(),
            filter: None,
            group_by: Vec::new(),
            having: None,
            order: Vec::new(),
            limit: None,
            offset: None,
            set: None,
        }),
        QueryStatement::Insert(statement) => {
            if statement.table.trim().is_empty() {
                return Err(CassieError::Planner(
                    "INSERT requires a target table".into(),
                ));
            }

            Ok(LogicalPlan {
                command: Some(LogicalCommand::Insert(statement.clone())),
                source: QuerySource::Collection(statement.table.clone()),
                collection: statement.table.clone(),
                ctes: Vec::new(),
                distinct: false,
                projection: Vec::new(),
                filter: None,
                group_by: Vec::new(),
                having: None,
                order: Vec::new(),
                limit: None,
                offset: Some(0),
                set: None,
            })
        }
        QueryStatement::Update(statement) => {
            if statement.table.trim().is_empty() {
                return Err(CassieError::Planner(
                    "UPDATE requires a target table".into(),
                ));
            }

            Ok(LogicalPlan {
                command: Some(LogicalCommand::Update(statement.clone())),
                source: QuerySource::Collection(statement.table.clone()),
                collection: statement.table.clone(),
                ctes: Vec::new(),
                distinct: false,
                projection: Vec::new(),
                filter: None,
                group_by: Vec::new(),
                having: None,
                order: Vec::new(),
                limit: None,
                offset: Some(0),
                set: None,
            })
        }
        QueryStatement::Delete(statement) => {
            if statement.table.trim().is_empty() {
                return Err(CassieError::Planner(
                    "DELETE requires a target table".into(),
                ));
            }

            Ok(LogicalPlan {
                command: Some(LogicalCommand::Delete(statement.clone())),
                source: QuerySource::Collection(statement.table.clone()),
                collection: statement.table.clone(),
                ctes: Vec::new(),
                distinct: false,
                projection: Vec::new(),
                filter: None,
                group_by: Vec::new(),
                having: None,
                order: Vec::new(),
                limit: None,
                offset: Some(0),
                set: None,
            })
        }
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
                distinct: false,
                projection: Vec::new(),
                filter: None,
                group_by: Vec::new(),
                having: None,
                order: Vec::new(),
                limit: None,
                offset: Some(0),
                set: None,
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
                distinct: false,
                projection: Vec::new(),
                filter: None,
                group_by: Vec::new(),
                having: None,
                order: Vec::new(),
                limit: None,
                offset: Some(0),
                set: None,
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
                distinct: false,
                projection: Vec::new(),
                filter: None,
                group_by: Vec::new(),
                having: None,
                order: Vec::new(),
                limit: None,
                offset: Some(0),
                set: None,
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
                distinct: false,
                projection: Vec::new(),
                filter: None,
                group_by: Vec::new(),
                having: None,
                order: Vec::new(),
                limit: None,
                offset: Some(0),
                set: None,
            })
        }
        QueryStatement::CreateView(statement) => {
            if statement.name.trim().is_empty() {
                return Err(CassieError::Planner("CREATE VIEW requires a name".into()));
            }
            if statement.query.trim().is_empty() {
                return Err(CassieError::Planner(
                    "CREATE VIEW requires a query body".into(),
                ));
            }

            Ok(LogicalPlan {
                command: Some(LogicalCommand::CreateView(statement.clone())),
                source: QuerySource::Collection(statement.name.clone()),
                collection: statement.name.clone(),
                ctes: Vec::new(),
                distinct: false,
                projection: Vec::new(),
                filter: None,
                group_by: Vec::new(),
                having: None,
                order: Vec::new(),
                limit: None,
                offset: Some(0),
                set: None,
            })
        }
        QueryStatement::DropView(statement) => {
            if statement.name.trim().is_empty() {
                return Err(CassieError::Planner("DROP VIEW requires a name".into()));
            }

            Ok(LogicalPlan {
                command: Some(LogicalCommand::DropView(statement.clone())),
                source: QuerySource::Collection(statement.name.clone()),
                collection: statement.name.clone(),
                ctes: Vec::new(),
                distinct: false,
                projection: Vec::new(),
                filter: None,
                group_by: Vec::new(),
                having: None,
                order: Vec::new(),
                limit: None,
                offset: Some(0),
                set: None,
            })
        }
        QueryStatement::CreateRole(statement) => {
            if statement.name.trim().is_empty() {
                return Err(CassieError::Planner("CREATE ROLE requires a name".into()));
            }

            Ok(LogicalPlan {
                command: Some(LogicalCommand::CreateRole(statement.clone())),
                source: QuerySource::Collection(statement.name.clone()),
                collection: statement.name.clone(),
                ctes: Vec::new(),
                distinct: false,
                projection: Vec::new(),
                filter: None,
                group_by: Vec::new(),
                having: None,
                order: Vec::new(),
                limit: None,
                offset: Some(0),
                set: None,
            })
        }
        QueryStatement::AlterRole(statement) => {
            if statement.name.trim().is_empty() {
                return Err(CassieError::Planner("ALTER ROLE requires a name".into()));
            }

            Ok(LogicalPlan {
                command: Some(LogicalCommand::AlterRole(statement.clone())),
                source: QuerySource::Collection(statement.name.clone()),
                collection: statement.name.clone(),
                ctes: Vec::new(),
                distinct: false,
                projection: Vec::new(),
                filter: None,
                group_by: Vec::new(),
                having: None,
                order: Vec::new(),
                limit: None,
                offset: Some(0),
                set: None,
            })
        }
        QueryStatement::DropRole(statement) => {
            if statement.name.trim().is_empty() {
                return Err(CassieError::Planner("DROP ROLE requires a name".into()));
            }

            Ok(LogicalPlan {
                command: Some(LogicalCommand::DropRole(statement.clone())),
                source: QuerySource::Collection(statement.name.clone()),
                collection: statement.name.clone(),
                ctes: Vec::new(),
                distinct: false,
                projection: Vec::new(),
                filter: None,
                group_by: Vec::new(),
                having: None,
                order: Vec::new(),
                limit: None,
                offset: Some(0),
                set: None,
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
                distinct: false,
                projection: Vec::new(),
                filter: None,
                group_by: Vec::new(),
                having: None,
                order: Vec::new(),
                limit: None,
                offset: Some(0),
                set: None,
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
                distinct: false,
                projection: Vec::new(),
                filter: None,
                group_by: Vec::new(),
                having: None,
                order: Vec::new(),
                limit: None,
                offset: Some(0),
                set: None,
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
                distinct: false,
                projection: Vec::new(),
                filter: None,
                group_by: Vec::new(),
                having: None,
                order: Vec::new(),
                limit: None,
                offset: Some(0),
                set: None,
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
                distinct: false,
                projection: Vec::new(),
                filter: None,
                group_by: Vec::new(),
                having: None,
                order: Vec::new(),
                limit: None,
                offset: Some(0),
                set: None,
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
                distinct: false,
                projection: Vec::new(),
                filter: None,
                group_by: Vec::new(),
                having: None,
                order: Vec::new(),
                limit: None,
                offset: Some(0),
                set: None,
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
                distinct: false,
                projection: Vec::new(),
                filter: None,
                group_by: Vec::new(),
                having: None,
                order: Vec::new(),
                limit: None,
                offset: Some(0),
                set: None,
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
                distinct: false,
                projection: Vec::new(),
                filter: None,
                group_by: Vec::new(),
                having: None,
                order: Vec::new(),
                limit: None,
                offset: Some(0),
                set: None,
            })
        }
        QueryStatement::Transaction(_) => Err(CassieError::Planner(
            "transaction control statements are handled by the session runtime".into(),
        )),
    }
}

fn source_name(source: &QuerySource) -> String {
    match source {
        QuerySource::Collection(name) | QuerySource::Cte(name) => name.clone(),
        QuerySource::SingleRow => "single_row".to_string(),
        QuerySource::Subquery { alias, .. } => alias.clone(),
        QuerySource::Join { .. } => "join".to_string(),
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
