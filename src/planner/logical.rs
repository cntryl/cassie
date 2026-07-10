use crate::app::CassieError;
use crate::sql::{
    ast::{
        AlterRetentionPolicyStatement, AlterRoleStatement, AlterSchemaStatement,
        AlterTableOperation, AlterTableStatement, CatalogStatementRef, CommonTableExpression,
        CreateDatabaseStatement, CreateFunctionStatement, CreateGraphStatement,
        CreateIndexStatement, CreateProcedureStatement, CreateRoleStatement, CreateSchemaStatement,
        CreateTableStatement, CreateViewStatement, DeleteStatement, DropDatabaseStatement,
        DropFunctionStatement, DropIndexStatement, DropMaterializedProjectionStatement,
        DropProcedureStatement, DropRetentionPolicyStatement, DropRoleStatement,
        DropRollupStatement, DropSchemaStatement, DropTableStatement, DropViewStatement,
        EnforceRetentionPolicyStatement, Expr, InsertStatement, OrderExpr, ProjectionStatementRef,
        QuerySource, RefreshRollupStatement, RetentionStatementRef, RuntimeStatementRef,
        SelectItem, SelectStatement, SetStatement, ShowStatement, StatementRouteRef,
        UpdateStatement, VerifyProjectionStatement,
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
    CreateGraph(CreateGraphStatement),
    DropTable(DropTableStatement),
    AlterTable(AlterTableStatement),
    CreateSequence(crate::sql::ast::CreateSequenceStatement),
    DropSequence(crate::sql::ast::DropSequenceStatement),
    CreateDatabase(CreateDatabaseStatement),
    DropDatabase(DropDatabaseStatement),
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
    CreateMaterializedProjection(crate::sql::ast::CreateMaterializedProjectionStatement),
    RefreshMaterializedProjection(crate::sql::ast::RefreshMaterializedProjectionStatement),
    DropMaterializedProjection(DropMaterializedProjectionStatement),
    AlterMaterializedProjection(crate::sql::ast::AlterMaterializedProjectionStatement),
    DropMaterializedProjectionVersion(crate::sql::ast::DropMaterializedProjectionVersionStatement),
    VerifyProjection(VerifyProjectionStatement),
    DiffProjection(crate::sql::ast::DiffProjectionStatement),
    CompareProjection(crate::sql::ast::CompareProjectionStatement),
    PlanRepairProjection(crate::sql::ast::PlanRepairProjectionStatement),
    RepairProjection(crate::sql::ast::RepairProjectionStatement),
    CreateRetentionPolicy(crate::sql::ast::CreateRetentionPolicyStatement),
    AlterRetentionPolicy(AlterRetentionPolicyStatement),
    DropRetentionPolicy(DropRetentionPolicyStatement),
    EnforceRetentionPolicy(EnforceRetentionPolicyStatement),
    CallProcedure(crate::sql::ast::CallProcedureStatement),
    Show(ShowStatement),
    Set(SetStatement),
    Copy(crate::sql::ast::CopyStatement),
    Insert(InsertStatement),
    Update(UpdateStatement),
    Delete(DeleteStatement),
}

/// # Errors
///
/// Returns an error when validation, storage, or execution fails.
pub fn plan(bound: &BoundStatement) -> Result<LogicalPlan, CassieError> {
    match bound.statement.statement.route() {
        StatementRouteRef::Runtime(statement) => plan_runtime_statement(statement),
        StatementRouteRef::Catalog(statement) => plan_catalog_statement(statement),
        StatementRouteRef::Projection(statement) => plan_projection_statement(statement),
        StatementRouteRef::Retention(statement) => plan_retention_statement(statement),
    }
}

fn plan_runtime_statement(statement: RuntimeStatementRef<'_>) -> Result<LogicalPlan, CassieError> {
    match statement {
        RuntimeStatementRef::Explain(_) => Err(CassieError::Planner(
            "EXPLAIN is handled before logical planning".to_string(),
        )),
        RuntimeStatementRef::Select(select) => plan_select(select),
        RuntimeStatementRef::Show(statement) => Ok(single_row_command_plan(LogicalCommand::Show(
            statement.clone(),
        ))),
        RuntimeStatementRef::Set(statement) => Ok(single_row_command_plan(LogicalCommand::Set(
            statement.clone(),
        ))),
        RuntimeStatementRef::Copy(statement) => plan_table_command(
            &statement.table,
            "COPY requires a target table",
            LogicalCommand::Copy(statement.clone()),
        ),
        RuntimeStatementRef::Insert(statement) => plan_table_command(
            &statement.table,
            "INSERT requires a target table",
            LogicalCommand::Insert(statement.clone()),
        ),
        RuntimeStatementRef::Update(statement) => plan_table_command(
            &statement.table,
            "UPDATE requires a target table",
            LogicalCommand::Update(statement.clone()),
        ),
        RuntimeStatementRef::Delete(statement) => plan_table_command(
            &statement.table,
            "DELETE requires a target table",
            LogicalCommand::Delete(statement.clone()),
        ),
        RuntimeStatementRef::Transaction(_) => Err(CassieError::Planner(
            "transaction control statements are handled by the session runtime".into(),
        )),
    }
}

fn plan_catalog_statement(statement: CatalogStatementRef<'_>) -> Result<LogicalPlan, CassieError> {
    match statement {
        CatalogStatementRef::CreateTable(statement) => plan_create_table(statement),
        CatalogStatementRef::CreateGraph(statement) => plan_named_command(
            &statement.name,
            "CREATE GRAPH requires a name",
            LogicalCommand::CreateGraph(statement.clone()),
        ),
        CatalogStatementRef::DropTable(statement) => plan_table_command(
            &statement.table,
            "DROP TABLE requires a table name",
            LogicalCommand::DropTable(statement.clone()),
        ),
        CatalogStatementRef::AlterTable(statement) => plan_alter_table(statement),
        CatalogStatementRef::CreateSequence(statement) => plan_named_command(
            &statement.name,
            "CREATE SEQUENCE requires a name",
            LogicalCommand::CreateSequence(statement.clone()),
        ),
        CatalogStatementRef::DropSequence(statement) => plan_named_command(
            &statement.name,
            "DROP SEQUENCE requires a name",
            LogicalCommand::DropSequence(statement.clone()),
        ),
        CatalogStatementRef::CreateDatabase(statement) => plan_named_command(
            &statement.name,
            "CREATE DATABASE requires a name",
            LogicalCommand::CreateDatabase(statement.clone()),
        ),
        CatalogStatementRef::DropDatabase(statement) => plan_named_command(
            &statement.name,
            "DROP DATABASE requires a name",
            LogicalCommand::DropDatabase(statement.clone()),
        ),
        CatalogStatementRef::CreateSchema(statement) => plan_named_command(
            &statement.schema,
            "CREATE SCHEMA requires a name",
            LogicalCommand::CreateSchema(statement.clone()),
        ),
        CatalogStatementRef::DropSchema(statement) => plan_named_command(
            &statement.schema,
            "DROP SCHEMA requires a schema name",
            LogicalCommand::DropSchema(statement.clone()),
        ),
        CatalogStatementRef::AlterSchema(statement) => plan_named_command(
            &statement.schema,
            "ALTER SCHEMA requires a schema name",
            LogicalCommand::AlterSchema(statement.clone()),
        ),
        CatalogStatementRef::CreateView(statement) => plan_create_view(statement),
        CatalogStatementRef::DropView(statement) => plan_named_command(
            &statement.name,
            "DROP VIEW requires a name",
            LogicalCommand::DropView(statement.clone()),
        ),
        CatalogStatementRef::CreateRole(statement) => plan_named_command(
            &statement.name,
            "CREATE ROLE requires a name",
            LogicalCommand::CreateRole(statement.clone()),
        ),
        CatalogStatementRef::AlterRole(statement) => plan_named_command(
            &statement.name,
            "ALTER ROLE requires a name",
            LogicalCommand::AlterRole(statement.clone()),
        ),
        CatalogStatementRef::DropRole(statement) => plan_named_command(
            &statement.name,
            "DROP ROLE requires a name",
            LogicalCommand::DropRole(statement.clone()),
        ),
        CatalogStatementRef::CreateFunction(statement) => plan_named_command(
            &statement.name,
            "CREATE FUNCTION requires a name",
            LogicalCommand::CreateFunction(statement.clone()),
        ),
        CatalogStatementRef::DropFunction(statement) => plan_named_command(
            &statement.name,
            "DROP FUNCTION requires a name",
            LogicalCommand::DropFunction(statement.clone()),
        ),
        CatalogStatementRef::CreateProcedure(statement) => plan_named_command(
            &statement.name,
            "CREATE PROCEDURE requires a name",
            LogicalCommand::CreateProcedure(statement.clone()),
        ),
        CatalogStatementRef::DropProcedure(statement) => plan_named_command(
            &statement.name,
            "DROP PROCEDURE requires a name",
            LogicalCommand::DropProcedure(statement.clone()),
        ),
        CatalogStatementRef::CallProcedure(statement) => plan_named_command(
            &statement.name,
            "CALL requires a procedure name",
            LogicalCommand::CallProcedure(statement.clone()),
        ),
        CatalogStatementRef::CreateIndex(statement) => plan_create_index(statement),
        CatalogStatementRef::DropIndex(statement) => plan_drop_index(statement),
    }
}

fn plan_projection_statement(
    statement: ProjectionStatementRef<'_>,
) -> Result<LogicalPlan, CassieError> {
    match statement {
        ProjectionStatementRef::CreateRollup(statement) => plan_named_command(
            &statement.name,
            "CREATE ROLLUP requires a name",
            LogicalCommand::CreateRollup(statement.clone()),
        ),
        ProjectionStatementRef::RefreshRollup(statement) => plan_named_command(
            &statement.name,
            "REFRESH ROLLUP requires a name",
            LogicalCommand::RefreshRollup(statement.clone()),
        ),
        ProjectionStatementRef::DropRollup(statement) => plan_named_command(
            &statement.name,
            "DROP ROLLUP requires a name",
            LogicalCommand::DropRollup(statement.clone()),
        ),
        ProjectionStatementRef::CreateMaterializedProjection(statement) => plan_named_command(
            &statement.name,
            "CREATE MATERIALIZED PROJECTION requires a name",
            LogicalCommand::CreateMaterializedProjection(statement.clone()),
        ),
        ProjectionStatementRef::RefreshMaterializedProjection(statement) => plan_named_command(
            &statement.name,
            "REFRESH MATERIALIZED PROJECTION requires a name",
            LogicalCommand::RefreshMaterializedProjection(statement.clone()),
        ),
        ProjectionStatementRef::DropMaterializedProjection(statement) => plan_named_command(
            &statement.name,
            "DROP MATERIALIZED PROJECTION requires a name",
            LogicalCommand::DropMaterializedProjection(statement.clone()),
        ),
        ProjectionStatementRef::AlterMaterializedProjection(statement) => plan_named_command(
            &statement.name,
            "ALTER MATERIALIZED PROJECTION requires a name",
            LogicalCommand::AlterMaterializedProjection(statement.clone()),
        ),
        ProjectionStatementRef::DropMaterializedProjectionVersion(statement) => plan_named_command(
            &statement.name,
            "DROP MATERIALIZED PROJECTION VERSION requires a name",
            LogicalCommand::DropMaterializedProjectionVersion(statement.clone()),
        ),
        ProjectionStatementRef::VerifyProjection(statement) => plan_named_command(
            &statement.name,
            "VERIFY PROJECTION requires a name",
            LogicalCommand::VerifyProjection(statement.clone()),
        ),
        ProjectionStatementRef::DiffProjection(statement) => plan_named_command(
            &statement.left.name,
            "DIFF PROJECTION requires a name",
            LogicalCommand::DiffProjection(statement.clone()),
        ),
        ProjectionStatementRef::CompareProjection(statement) => plan_named_command(
            &statement.target.name,
            "COMPARE PROJECTION requires a name",
            LogicalCommand::CompareProjection(statement.clone()),
        ),
        ProjectionStatementRef::PlanRepairProjection(statement) => plan_named_command(
            &statement.target.name,
            "PLAN REPAIR PROJECTION requires a name",
            LogicalCommand::PlanRepairProjection(statement.clone()),
        ),
        ProjectionStatementRef::RepairProjection(statement) => plan_named_command(
            &statement.target.name,
            "REPAIR PROJECTION requires a name",
            LogicalCommand::RepairProjection(statement.clone()),
        ),
    }
}

fn plan_retention_statement(
    statement: RetentionStatementRef<'_>,
) -> Result<LogicalPlan, CassieError> {
    match statement {
        RetentionStatementRef::CreateRetentionPolicy(statement) => plan_named_command(
            &statement.name,
            "CREATE RETENTION POLICY requires a name",
            LogicalCommand::CreateRetentionPolicy(statement.clone()),
        ),
        RetentionStatementRef::AlterRetentionPolicy(statement) => plan_named_command(
            &statement.name,
            "ALTER RETENTION POLICY requires a name",
            LogicalCommand::AlterRetentionPolicy(statement.clone()),
        ),
        RetentionStatementRef::DropRetentionPolicy(statement) => plan_named_command(
            &statement.name,
            "DROP RETENTION POLICY requires a name",
            LogicalCommand::DropRetentionPolicy(statement.clone()),
        ),
        RetentionStatementRef::EnforceRetentionPolicy(statement) => plan_named_command(
            &statement.name,
            "ENFORCE RETENTION POLICY requires a name",
            LogicalCommand::EnforceRetentionPolicy(statement.clone()),
        ),
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
    if (statement.fields.is_empty() && statement.expressions.is_empty())
        || statement.fields.iter().any(|field| field.trim().is_empty())
    {
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
        QuerySource::Collection(name)
        | QuerySource::Cte(name)
        | QuerySource::TableFunction { name, .. } => name.clone(),
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
        AlterTableOperation::AddConstraint { constraints } => {
            if constraints.is_empty()
                || constraints
                    .iter()
                    .any(|constraint| constraint.field.trim().is_empty())
            {
                return Err(CassieError::Planner(
                    "ALTER TABLE ADD CONSTRAINT requires a field".into(),
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
        AlterTableOperation::AlterColumnSetDefault { field, .. }
        | AlterTableOperation::AlterColumnDropDefault { field }
        | AlterTableOperation::AlterColumnSetNotNull { field }
        | AlterTableOperation::AlterColumnDropNotNull { field } => {
            if field.trim().is_empty() {
                return Err(CassieError::Planner(
                    "ALTER TABLE ALTER COLUMN requires a field".into(),
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
