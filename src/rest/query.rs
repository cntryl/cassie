use crate::app::{Cassie, CassieError, CassieSession, QueryExplainOutput, QueryExplainPlan};
use crate::catalog::IndexKind;
use crate::executor::{ColumnMeta, QueryResult};
use crate::runtime::QueryCancellationHandle;
use crate::sql::ast::{QueryStatement, TransactionAction};
use crate::types::Value;

#[derive(serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct QueryExecuteRequest {
    pub sql: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct QueryValidateRequest {
    pub sql: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct QueryExplainRequest {
    pub sql: String,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct RestColumnMeta {
    pub name: String,
    pub data_type: String,
    pub type_oid: i64,
    pub typlen: i16,
    pub atttypmod: i32,
    pub format_code: i16,
    pub nullable: bool,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct RestQueryResult {
    pub columns: Vec<RestColumnMeta>,
    pub rows: Vec<Vec<serde_json::Value>>,
    pub command: String,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct RestQueryExplainResponse {
    pub columns: Vec<RestColumnMeta>,
    pub rows: Vec<Vec<serde_json::Value>>,
    pub command: String,
    pub plan: QueryExplainPlan,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct QueryValidateResponse {
    pub valid: bool,
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub columns: Option<Vec<RestColumnMeta>>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct QuerySchemaResponse {
    pub sections: Vec<QuerySchemaSection>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct QuerySchemaSection {
    pub id: String,
    pub label: String,
    pub items: Vec<QuerySchemaItem>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct QuerySchemaItem {
    pub id: String,
    pub kind: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<String>,
}

/// # Errors
///
/// Returns an error when the request body is invalid or SQL execution fails.
pub fn execute(cassie: &Cassie, user: &str, body: &[u8]) -> Result<RestQueryResult, CassieError> {
    let session = cassie.create_session(user, None);
    execute_with_session(cassie, &session, body)
}

pub(crate) fn execute_with_session(
    cassie: &Cassie,
    session: &CassieSession,
    body: &[u8],
) -> Result<RestQueryResult, CassieError> {
    execute_with_session_and_cancellation(cassie, session, body, &QueryCancellationHandle::new())
}

#[doc(hidden)]
pub fn execute_with_session_and_cancellation(
    cassie: &Cassie,
    session: &CassieSession,
    body: &[u8],
    cancellation: &QueryCancellationHandle,
) -> Result<RestQueryResult, CassieError> {
    let request: QueryExecuteRequest =
        serde_json::from_slice(body).map_err(|error| CassieError::Parse(error.to_string()))?;
    cassie
        .execute_sql_with_cancellation(session, request.sql.as_str(), Vec::new(), cancellation)
        .map(RestQueryResult::from)
}

/// # Errors
///
/// Returns an error when the request body is invalid or SQL validation fails.
pub fn validate(cassie: &Cassie, body: &[u8]) -> Result<QueryValidateResponse, CassieError> {
    let session = cassie.create_session(&cassie.auth_user, None);
    validate_with_session(cassie, &session, body)
}

pub(crate) fn validate_with_session(
    cassie: &Cassie,
    session: &CassieSession,
    body: &[u8],
) -> Result<QueryValidateResponse, CassieError> {
    let request: QueryValidateRequest =
        serde_json::from_slice(body).map_err(|error| CassieError::Parse(error.to_string()))?;
    let parsed = crate::sql::parse_statement(request.sql.as_str())?;
    session.authorize_statement(&parsed.statement)?;
    let command = command_name(&parsed.statement).to_string();
    let fingerprint = crate::runtime::sql_fingerprint(&parsed);
    let columns = cassie
        .describe_parsed_statement(parsed, fingerprint)?
        .into_iter()
        .map(RestColumnMeta::from)
        .collect();

    Ok(QueryValidateResponse {
        valid: true,
        command,
        columns: Some(columns),
    })
}

/// # Errors
///
/// Returns an error when the request body is invalid or SQL planning fails.
pub fn explain(
    cassie: &Cassie,
    user: &str,
    body: &[u8],
) -> Result<RestQueryExplainResponse, CassieError> {
    let session = cassie.create_session(user, None);
    explain_with_session(cassie, &session, body)
}

pub(crate) fn explain_with_session(
    cassie: &Cassie,
    session: &CassieSession,
    body: &[u8],
) -> Result<RestQueryExplainResponse, CassieError> {
    let request: QueryExplainRequest =
        serde_json::from_slice(body).map_err(|error| CassieError::Parse(error.to_string()))?;
    cassie
        .explain_sql(session, request.sql.as_str(), Vec::new())
        .map(RestQueryExplainResponse::from)
}

#[must_use]
pub fn schema(cassie: &Cassie) -> QuerySchemaResponse {
    QuerySchemaResponse {
        sections: vec![
            section("tables", "Tables", table_items(cassie)),
            section("views", "Views", view_items(cassie)),
            section("indexes", "Indexes", index_items(cassie)),
            section("udfs", "UDFs", function_items(cassie)),
            section("procedures", "Procedures", procedure_items(cassie)),
        ],
    }
}

fn section(id: &str, label: &str, items: Vec<QuerySchemaItem>) -> QuerySchemaSection {
    QuerySchemaSection {
        id: id.to_string(),
        label: label.to_string(),
        items,
    }
}

fn table_items(cassie: &Cassie) -> Vec<QuerySchemaItem> {
    let mut items: Vec<QuerySchemaItem> = cassie
        .catalog
        .list_collections_canonical()
        .into_iter()
        .map(|collection| {
            let column_count = cassie
                .catalog
                .get_schema(collection.name.as_str())
                .map_or(0, |schema| schema.fields.len());
            QuerySchemaItem {
                id: format!("table:{}", collection.name),
                kind: "table".to_string(),
                label: collection.name,
                metadata: Some(column_count_label(column_count)),
            }
        })
        .collect();

    items.sort_by_key(|item| item.label.to_ascii_lowercase());
    items
}

fn view_items(cassie: &Cassie) -> Vec<QuerySchemaItem> {
    let mut items: Vec<QuerySchemaItem> = cassie
        .catalog
        .list_views()
        .into_iter()
        .map(|view| QuerySchemaItem {
            id: format!("view:{}", view.name),
            kind: "view".to_string(),
            label: view.name,
            metadata: Some(column_count_label(view.schema.fields.len())),
        })
        .collect();

    items.sort_by_key(|item| item.label.to_ascii_lowercase());
    items
}

fn index_items(cassie: &Cassie) -> Vec<QuerySchemaItem> {
    let mut items = Vec::new();
    for collection in cassie.catalog.list_collections_canonical() {
        for index in cassie.catalog.list_indexes(collection.name.as_str()) {
            let fields = if index.normalized_fields().is_empty() {
                index.normalized_expressions().join(", ")
            } else {
                index.normalized_fields().join(", ")
            };
            items.push(QuerySchemaItem {
                id: format!("index:{}:{}", index.collection, index.name),
                kind: "index".to_string(),
                label: index.name,
                metadata: Some(format!(
                    "{} on {}({})",
                    index_kind_label(&index.kind),
                    index.collection,
                    fields
                )),
            });
        }
    }
    items.sort_by_key(|item| item.label.to_ascii_lowercase());
    items
}

fn function_items(cassie: &Cassie) -> Vec<QuerySchemaItem> {
    let mut items: Vec<QuerySchemaItem> = cassie
        .catalog
        .list_functions()
        .into_iter()
        .map(|function| {
            let args = function
                .args
                .iter()
                .map(|arg| format!("{} {}", arg.name, arg.data_type.type_name()))
                .collect::<Vec<_>>()
                .join(", ");
            QuerySchemaItem {
                id: format!("udf:{}", function.name),
                kind: "udf".to_string(),
                label: function.name,
                metadata: Some(format!("({args}) -> {}", function.return_type.type_name())),
            }
        })
        .collect();

    items.sort_by_key(|item| item.label.to_ascii_lowercase());
    items
}

fn procedure_items(cassie: &Cassie) -> Vec<QuerySchemaItem> {
    let mut items: Vec<QuerySchemaItem> = cassie
        .catalog
        .list_procedures()
        .into_iter()
        .map(|procedure| {
            let args = procedure
                .args
                .iter()
                .map(|arg| format!("{} {}", arg.name, arg.data_type.type_name()))
                .collect::<Vec<_>>()
                .join(", ");
            QuerySchemaItem {
                id: format!("procedure:{}", procedure.name),
                kind: "procedure".to_string(),
                label: procedure.name,
                metadata: Some(format!("({args})")),
            }
        })
        .collect();

    items.sort_by_key(|item| item.label.to_ascii_lowercase());
    items
}

fn column_count_label(column_count: usize) -> String {
    if column_count == 1 {
        "1 column".to_string()
    } else {
        format!("{column_count} columns")
    }
}

fn index_kind_label(kind: &IndexKind) -> &'static str {
    match kind {
        IndexKind::Scalar => "scalar",
        IndexKind::FullText => "fulltext",
        IndexKind::Vector => "vector",
        IndexKind::Hybrid => "hybrid",
        IndexKind::Column => "column",
        IndexKind::TimeSeries => "time_series",
    }
}

fn command_name(statement: &QueryStatement) -> &'static str {
    match statement {
        QueryStatement::Explain(_) => "EXPLAIN",
        QueryStatement::Select(_) => "SELECT",
        QueryStatement::Show(_) => "SHOW",
        QueryStatement::Set(_) => "SET",
        QueryStatement::Copy(_) => "COPY",
        QueryStatement::Insert(_) => "INSERT",
        QueryStatement::Update(_) => "UPDATE",
        QueryStatement::Delete(_) => "DELETE",
        QueryStatement::Transaction(statement) => transaction_command_name(&statement.action),
        QueryStatement::CreateTable(_) => "CREATE TABLE",
        QueryStatement::CreateGraph(_) => "CREATE GRAPH",
        QueryStatement::DropTable(_) => "DROP TABLE",
        QueryStatement::AlterTable(_) => "ALTER TABLE",
        QueryStatement::CreateSequence(_) => "CREATE SEQUENCE",
        QueryStatement::DropSequence(_) => "DROP SEQUENCE",
        QueryStatement::CreateDatabase(_) => "CREATE DATABASE",
        QueryStatement::DropDatabase(_) => "DROP DATABASE",
        QueryStatement::CreateSchema(_) => "CREATE SCHEMA",
        QueryStatement::CreateView(_) => "CREATE VIEW",
        QueryStatement::DropView(_) => "DROP VIEW",
        QueryStatement::CreateRole(_) => "CREATE ROLE",
        QueryStatement::AlterRole(_) => "ALTER ROLE",
        QueryStatement::DropRole(_) => "DROP ROLE",
        QueryStatement::CreateIndex(_) => "CREATE INDEX",
        QueryStatement::DropIndex(_) => "DROP INDEX",
        QueryStatement::CreateRollup(_) => "CREATE ROLLUP",
        QueryStatement::RefreshRollup(_) => "REFRESH ROLLUP",
        QueryStatement::DropRollup(_) => "DROP ROLLUP",
        QueryStatement::CreateMaterializedProjection(_) => "CREATE MATERIALIZED PROJECTION",
        QueryStatement::RefreshMaterializedProjection(_) => "REFRESH MATERIALIZED PROJECTION",
        QueryStatement::DropMaterializedProjection(_) => "DROP MATERIALIZED PROJECTION",
        QueryStatement::AlterMaterializedProjection(_) => "ALTER MATERIALIZED PROJECTION",
        QueryStatement::DropMaterializedProjectionVersion(_) => {
            "DROP MATERIALIZED PROJECTION VERSION"
        }
        QueryStatement::VerifyProjection(_) => "VERIFY PROJECTION",
        QueryStatement::DiffProjection(_) => "DIFF PROJECTION",
        QueryStatement::CompareProjection(_) => "COMPARE PROJECTION",
        QueryStatement::PlanRepairProjection(_) => "PLAN REPAIR PROJECTION",
        QueryStatement::RepairProjection(_) => "REPAIR PROJECTION",
        QueryStatement::CreateRetentionPolicy(_) => "CREATE RETENTION POLICY",
        QueryStatement::AlterRetentionPolicy(_) => "ALTER RETENTION POLICY",
        QueryStatement::DropRetentionPolicy(_) => "DROP RETENTION POLICY",
        QueryStatement::EnforceRetentionPolicy(_) => "ENFORCE RETENTION POLICY",
        QueryStatement::CreateFunction(_) => "CREATE FUNCTION",
        QueryStatement::DropFunction(_) => "DROP FUNCTION",
        QueryStatement::CreateProcedure(_) => "CREATE PROCEDURE",
        QueryStatement::DropProcedure(_) => "DROP PROCEDURE",
        QueryStatement::CallProcedure(_) => "CALL",
        QueryStatement::DropSchema(_) => "DROP SCHEMA",
        QueryStatement::AlterSchema(_) => "ALTER SCHEMA",
    }
}

fn transaction_command_name(action: &TransactionAction) -> &'static str {
    match action {
        TransactionAction::Begin => "BEGIN",
        TransactionAction::Commit => "COMMIT",
        TransactionAction::Rollback => "ROLLBACK",
        TransactionAction::Savepoint { .. } => "SAVEPOINT",
        TransactionAction::RollbackTo { .. } => "ROLLBACK TO SAVEPOINT",
        TransactionAction::Release { .. } => "RELEASE SAVEPOINT",
    }
}

impl From<ColumnMeta> for RestColumnMeta {
    fn from(column: ColumnMeta) -> Self {
        Self {
            name: column.name,
            data_type: column.data_type,
            type_oid: column.type_oid,
            typlen: column.typlen,
            atttypmod: column.atttypmod,
            format_code: column.format_code,
            nullable: column.nullable,
        }
    }
}

impl From<QueryResult> for RestQueryResult {
    fn from(result: QueryResult) -> Self {
        Self {
            columns: result
                .columns
                .into_iter()
                .map(RestColumnMeta::from)
                .collect(),
            rows: result
                .rows
                .into_iter()
                .map(|row| row.into_iter().map(query_value_to_json).collect())
                .collect(),
            command: result.command,
        }
    }
}

impl From<QueryExplainOutput> for RestQueryExplainResponse {
    fn from(output: QueryExplainOutput) -> Self {
        let result = RestQueryResult::from(output.result);
        Self {
            columns: result.columns,
            rows: result.rows,
            command: result.command,
            plan: output.plan,
        }
    }
}

fn query_value_to_json(value: Value) -> serde_json::Value {
    match value {
        Value::Null => serde_json::Value::Null,
        Value::Bool(value) => serde_json::Value::Bool(value),
        Value::Int64(value) => serde_json::Value::Number(value.into()),
        Value::Float64(value) => serde_json::Number::from_f64(value)
            .map_or(serde_json::Value::Null, serde_json::Value::Number),
        Value::String(value) => serde_json::Value::String(value),
        Value::Vector(value) => serde_json::Value::Array(
            value
                .values
                .into_iter()
                .map(|value| {
                    serde_json::Number::from_f64(f64::from(value))
                        .map_or(serde_json::Value::Null, serde_json::Value::Number)
                })
                .collect(),
        ),
        Value::Json(value) => value,
    }
}
