use crate::app::CassieError;
use crate::sql::{
    ast::{
        AlterRoleStatement, AlterSchemaStatement, AlterTableOperation, AlterTableStatement,
        CommonTableExpression, CreateFunctionStatement, CreateIndexStatement,
        CreateProcedureStatement, CreateRoleStatement, CreateSchemaStatement, CreateTableStatement,
        CreateViewStatement, DeleteStatement, DropFunctionStatement, DropIndexStatement,
        DropProcedureStatement, DropRoleStatement, DropRollupStatement, DropSchemaStatement,
        DropTableStatement, DropViewStatement, Expr, InsertStatement, OrderExpr, QuerySource,
        QueryStatement, RefreshRollupStatement, SelectItem, SelectStatement, SetStatement,
        ShowStatement, UpdateStatement,
    },
    binder::BoundStatement,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogicalPlan {
    pub command: Option<LogicalCommand>,
    pub source: QuerySource,
    pub collection: String,
    pub ctes: Vec<CommonTableExpression>,
    pub distinct: bool,
    pub distinct_on: Vec<Expr>,
    pub projection: Vec<SelectItem>,
    pub filter: Option<Expr>,
    pub group_by: Vec<Expr>,
    pub having: Option<Expr>,
    pub order: Vec<OrderExpr>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub set: Option<Box<crate::sql::ast::SelectSet>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    DropSchema(DropSchemaStatement),
    AlterSchema(AlterSchemaStatement),
    CreateView(CreateViewStatement),
    DropView(DropViewStatement),
    CreateIndex(CreateIndexStatement),
    DropIndex(DropIndexStatement),
    CreateRollup(crate::sql::ast::CreateRollupStatement),
    RefreshRollup(RefreshRollupStatement),
    DropRollup(DropRollupStatement),
    CallProcedure(crate::sql::ast::CallProcedureStatement),
    Show(ShowStatement),
    Set(SetStatement),
    Insert(InsertStatement),
    Update(UpdateStatement),
    Delete(DeleteStatement),
}

pub fn plan(bound: &BoundStatement) -> Result<LogicalPlan, CassieError> {
    match &bound.statement.statement {
        QueryStatement::Explain(_) => Err(CassieError::Planner(
            "EXPLAIN is handled before logical planning".to_string(),
        )),
        QueryStatement::Select(select) => plan_select(select),
        QueryStatement::Show(statement) => Ok(single_row_command_plan(LogicalCommand::Show(
            statement.clone(),
        ))),
        QueryStatement::Set(statement) => Ok(single_row_command_plan(LogicalCommand::Set(
            statement.clone(),
        ))),
        QueryStatement::Insert(statement) => plan_table_command(
            &statement.table,
            "INSERT requires a target table",
            LogicalCommand::Insert(statement.clone()),
        ),
        QueryStatement::Update(statement) => plan_table_command(
            &statement.table,
            "UPDATE requires a target table",
            LogicalCommand::Update(statement.clone()),
        ),
        QueryStatement::Delete(statement) => plan_table_command(
            &statement.table,
            "DELETE requires a target table",
            LogicalCommand::Delete(statement.clone()),
        ),
        QueryStatement::CreateTable(statement) => plan_create_table(statement),
        QueryStatement::DropTable(statement) => plan_table_command(
            &statement.table,
            "DROP TABLE requires a table name",
            LogicalCommand::DropTable(statement.clone()),
        ),
        QueryStatement::AlterTable(statement) => plan_alter_table(statement),
        QueryStatement::CreateSchema(statement) => plan_named_command(
            &statement.schema,
            "CREATE SCHEMA requires a name",
            LogicalCommand::CreateSchema(statement.clone()),
        ),
        QueryStatement::DropSchema(statement) => plan_named_command(
            &statement.schema,
            "DROP SCHEMA requires a schema name",
            LogicalCommand::DropSchema(statement.clone()),
        ),
        QueryStatement::AlterSchema(statement) => plan_named_command(
            &statement.schema,
            "ALTER SCHEMA requires a schema name",
            LogicalCommand::AlterSchema(statement.clone()),
        ),
        QueryStatement::CreateView(statement) => plan_create_view(statement),
        QueryStatement::DropView(statement) => plan_named_command(
            &statement.name,
            "DROP VIEW requires a name",
            LogicalCommand::DropView(statement.clone()),
        ),
        QueryStatement::CreateRole(statement) => plan_named_command(
            &statement.name,
            "CREATE ROLE requires a name",
            LogicalCommand::CreateRole(statement.clone()),
        ),
        QueryStatement::AlterRole(statement) => plan_named_command(
            &statement.name,
            "ALTER ROLE requires a name",
            LogicalCommand::AlterRole(statement.clone()),
        ),
        QueryStatement::DropRole(statement) => plan_named_command(
            &statement.name,
            "DROP ROLE requires a name",
            LogicalCommand::DropRole(statement.clone()),
        ),
        QueryStatement::CreateFunction(statement) => plan_named_command(
            &statement.name,
            "CREATE FUNCTION requires a name",
            LogicalCommand::CreateFunction(statement.clone()),
        ),
        QueryStatement::DropFunction(statement) => plan_named_command(
            &statement.name,
            "DROP FUNCTION requires a name",
            LogicalCommand::DropFunction(statement.clone()),
        ),
        QueryStatement::CreateProcedure(statement) => plan_named_command(
            &statement.name,
            "CREATE PROCEDURE requires a name",
            LogicalCommand::CreateProcedure(statement.clone()),
        ),
        QueryStatement::DropProcedure(statement) => plan_named_command(
            &statement.name,
            "DROP PROCEDURE requires a name",
            LogicalCommand::DropProcedure(statement.clone()),
        ),
        QueryStatement::CallProcedure(statement) => plan_named_command(
            &statement.name,
            "CALL requires a procedure name",
            LogicalCommand::CallProcedure(statement.clone()),
        ),
        QueryStatement::CreateIndex(statement) => plan_create_index(statement),
        QueryStatement::DropIndex(statement) => plan_drop_index(statement),
        QueryStatement::CreateRollup(statement) => plan_named_command(
            &statement.name,
            "CREATE ROLLUP requires a name",
            LogicalCommand::CreateRollup(statement.clone()),
        ),
        QueryStatement::RefreshRollup(statement) => plan_named_command(
            &statement.name,
            "REFRESH ROLLUP requires a name",
            LogicalCommand::RefreshRollup(statement.clone()),
        ),
        QueryStatement::DropRollup(statement) => plan_named_command(
            &statement.name,
            "DROP ROLLUP requires a name",
            LogicalCommand::DropRollup(statement.clone()),
        ),
        QueryStatement::Transaction(_) => Err(CassieError::Planner(
            "transaction control statements are handled by the session runtime".into(),
        )),
    }
}

fn plan_select(select: &SelectStatement) -> Result<LogicalPlan, CassieError> {
    validate_logical_plan(select)?;
    Ok(LogicalPlan {
        command: None,
        source: select.source.clone(),
        collection: source_name(&select.source),
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
    })
}

fn single_row_command_plan(command: LogicalCommand) -> LogicalPlan {
    command_plan(command, QuerySource::SingleRow, String::new(), None)
}

fn plan_table_command(
    table: &str,
    missing_message: &'static str,
    command: LogicalCommand,
) -> Result<LogicalPlan, CassieError> {
    require_name(table, missing_message)?;
    Ok(command_plan(
        command,
        QuerySource::Collection(table.to_string()),
        table.to_string(),
        Some(0),
    ))
}

fn plan_named_command(
    name: &str,
    missing_message: &'static str,
    command: LogicalCommand,
) -> Result<LogicalPlan, CassieError> {
    require_name(name, missing_message)?;
    Ok(command_plan(
        command,
        QuerySource::Collection(name.to_string()),
        name.to_string(),
        Some(0),
    ))
}

fn plan_create_table(statement: &CreateTableStatement) -> Result<LogicalPlan, CassieError> {
    require_name(&statement.table, "CREATE TABLE requires a table name")?;
    if statement.fields.is_empty() {
        return Err(CassieError::Planner(
            "CREATE TABLE requires at least one column".into(),
        ));
    }
    plan_table_command(
        &statement.table,
        "CREATE TABLE requires a table name",
        LogicalCommand::CreateTable(statement.clone()),
    )
}

fn plan_alter_table(statement: &AlterTableStatement) -> Result<LogicalPlan, CassieError> {
    require_name(&statement.table, "ALTER TABLE requires a table name")?;
    validate_alter_command(statement)?;
    plan_table_command(
        &statement.table,
        "ALTER TABLE requires a table name",
        LogicalCommand::AlterTable(statement.clone()),
    )
}

fn plan_create_view(statement: &CreateViewStatement) -> Result<LogicalPlan, CassieError> {
    require_name(&statement.name, "CREATE VIEW requires a name")?;
    require_name(&statement.query, "CREATE VIEW requires a query body")?;
    plan_named_command(
        &statement.name,
        "CREATE VIEW requires a name",
        LogicalCommand::CreateView(statement.clone()),
    )
}

fn plan_create_index(statement: &CreateIndexStatement) -> Result<LogicalPlan, CassieError> {
    require_name(&statement.table, "CREATE INDEX requires a collection name")?;
    require_name(&statement.name, "CREATE INDEX requires an index name")?;
    if statement.fields.is_empty() || statement.fields.iter().any(|field| field.trim().is_empty()) {
        return Err(CassieError::Planner(
            "CREATE INDEX requires an indexed field".into(),
        ));
    }
    plan_table_command(
        &statement.table,
        "CREATE INDEX requires a collection name",
        LogicalCommand::CreateIndex(statement.clone()),
    )
}

fn plan_drop_index(statement: &DropIndexStatement) -> Result<LogicalPlan, CassieError> {
    require_name(&statement.table, "DROP INDEX requires a collection name")?;
    require_name(&statement.name, "DROP INDEX requires an index name")?;
    plan_table_command(
        &statement.table,
        "DROP INDEX requires a collection name",
        LogicalCommand::DropIndex(statement.clone()),
    )
}

fn command_plan(
    command: LogicalCommand,
    source: QuerySource,
    collection: String,
    offset: Option<i64>,
) -> LogicalPlan {
    LogicalPlan {
        command: Some(command),
        source,
        collection,
        ctes: Vec::new(),
        distinct: false,
        distinct_on: Vec::new(),
        projection: Vec::new(),
        filter: None,
        group_by: Vec::new(),
        having: None,
        order: Vec::new(),
        limit: None,
        offset,
        set: None,
    }
}

fn require_name(value: &str, message: &'static str) -> Result<(), CassieError> {
    if value.trim().is_empty() {
        return Err(CassieError::Planner(message.into()));
    }
    Ok(())
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
        AlterTableOperation::RenameColumn { from, to } => {
            if from.trim().is_empty() {
                return Err(CassieError::Planner(
                    "ALTER TABLE RENAME COLUMN requires a field".into(),
                ));
            }
            if to.trim().is_empty() {
                return Err(CassieError::Planner(
                    "ALTER TABLE RENAME COLUMN requires a target field".into(),
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
