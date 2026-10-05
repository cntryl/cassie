use crate::app::{Cassie, CassieError, CassieSession, QueryExplainOutput, QueryExplainPlan};
use crate::catalog::IndexKind;
use crate::catalog::RelationId;
use crate::executor::{ColumnMeta, QueryResult};
use crate::runtime::QueryCancellationHandle;
use crate::sql::ast::{QueryStatement, TransactionAction};
use crate::types::json::try_value_to_json;

#[derive(serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct QueryExecuteRequest {
    pub sql: String,
    pub operation_id: Option<uuid::Uuid>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct QueryValidateRequest {
    pub sql: String,
    pub operation_id: Option<uuid::Uuid>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct QueryExplainRequest {
    pub sql: String,
    pub operation_id: Option<uuid::Uuid>,
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
    pub database: String,
    pub schema: String,
    pub name: String,
    pub columns: Vec<QuerySchemaColumn>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<String>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct QuerySchemaColumn {
    pub id: String,
    pub name: String,
    pub data_type: String,
    pub primary_key: bool,
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
        .and_then(RestQueryResult::try_from)
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
    validate_with_session_and_cancellation(cassie, session, body, &QueryCancellationHandle::new())
}

pub(crate) fn validate_with_session_and_cancellation(
    cassie: &Cassie,
    session: &CassieSession,
    body: &[u8],
    cancellation: &QueryCancellationHandle,
) -> Result<QueryValidateResponse, CassieError> {
    let request: QueryValidateRequest =
        serde_json::from_slice(body).map_err(|error| CassieError::Parse(error.to_string()))?;
    if cancellation.is_cancelled() {
        return Err(CassieError::QueryCancelled);
    }
    let parsed = crate::sql::parse_statement(request.sql.as_str())?;
    session.authorize_statement(&parsed.statement)?;
    let command = command_name(&parsed.statement).to_string();
    let fingerprint = crate::runtime::sql_fingerprint(&parsed);
    let columns = cassie
        .describe_parsed_statement_for_session(session, parsed, fingerprint, &[])?
        .into_iter()
        .map(RestColumnMeta::from)
        .collect();

    if cancellation.is_cancelled() {
        return Err(CassieError::QueryCancelled);
    }
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
    explain_with_session_and_cancellation(cassie, session, body, &QueryCancellationHandle::new())
}

pub(crate) fn explain_with_session_and_cancellation(
    cassie: &Cassie,
    session: &CassieSession,
    body: &[u8],
    cancellation: &QueryCancellationHandle,
) -> Result<RestQueryExplainResponse, CassieError> {
    let request: QueryExplainRequest =
        serde_json::from_slice(body).map_err(|error| CassieError::Parse(error.to_string()))?;
    cassie
        .explain_sql_with_cancellation(session, request.sql.as_str(), Vec::new(), cancellation)
        .and_then(RestQueryExplainResponse::try_from)
}

#[must_use]
///
/// # Panics
///
/// Panics when persisted catalog object names violate the canonical
/// `database.schema.name` invariant.
pub fn schema(cassie: &Cassie) -> QuerySchemaResponse {
    schema_for_database(cassie, &cassie.default_database)
        .expect("catalog metadata must use canonical relation names")
}

pub(crate) fn schema_with_session(
    cassie: &Cassie,
    session: &CassieSession,
) -> Result<QuerySchemaResponse, CassieError> {
    cassie.ensure_session_database_access(session)?;
    let database = session
        .current_database()
        .unwrap_or(&cassie.default_database);
    schema_for_database(cassie, database)
}

fn schema_for_database(
    cassie: &Cassie,
    database: &str,
) -> Result<QuerySchemaResponse, CassieError> {
    Ok(QuerySchemaResponse {
        sections: vec![
            section("tables", "Tables", table_items(cassie, database)?),
            section("views", "Views", view_items(cassie, database)?),
            section("indexes", "Indexes", index_items(cassie, database)?),
            section("udfs", "UDFs", function_items(cassie, database)?),
            section(
                "procedures",
                "Procedures",
                procedure_items(cassie, database)?,
            ),
        ],
    })
}

fn section(id: &str, label: &str, items: Vec<QuerySchemaItem>) -> QuerySchemaSection {
    QuerySchemaSection {
        id: id.to_string(),
        label: label.to_string(),
        items,
    }
}

fn table_items(cassie: &Cassie, database: &str) -> Result<Vec<QuerySchemaItem>, CassieError> {
    let mut items: Vec<QuerySchemaItem> = cassie
        .catalog
        .list_collections_canonical()
        .into_iter()
        .map(|collection| {
            let relation = canonical_relation(&collection.name)?;
            let schema = cassie.catalog.get_schema(collection.name.as_str());
            let constraints = cassie.catalog.get_constraints(collection.name.as_str());
            let columns = schema.map_or_else(Vec::new, |schema| {
                schema
                    .fields
                    .iter()
                    .map(|field| QuerySchemaColumn {
                        id: format!("column:{}:{}", collection.name, field.name),
                        name: field.name.clone(),
                        data_type: field.data_type.type_name().clone(),
                        primary_key: constraints.iter().any(|constraint| {
                            constraint.field == field.name && constraint.primary_key
                        }),
                    })
                    .collect()
            });
            Ok(QuerySchemaItem {
                id: format!("table:{}", collection.name),
                kind: "table".to_string(),
                label: collection.name,
                database: relation.database,
                schema: relation.schema,
                name: relation.name,
                metadata: Some(column_count_label(columns.len())),
                columns,
            })
        })
        .collect::<Result<_, CassieError>>()?;

    items.retain(|item| item.database.eq_ignore_ascii_case(database));

    items.sort_by_key(|item| item.label.to_ascii_lowercase());
    Ok(items)
}

fn view_items(cassie: &Cassie, database: &str) -> Result<Vec<QuerySchemaItem>, CassieError> {
    let mut items: Vec<QuerySchemaItem> = cassie
        .catalog
        .list_views()
        .into_iter()
        .map(|view| {
            let relation = canonical_relation(&view.name)?;
            let columns = view
                .schema
                .fields
                .iter()
                .map(|field| QuerySchemaColumn {
                    id: format!("column:{}:{}", view.name, field.name),
                    name: field.name.clone(),
                    data_type: field.data_type.type_name().clone(),
                    primary_key: false,
                })
                .collect::<Vec<_>>();
            Ok(QuerySchemaItem {
                id: format!("view:{}", view.name),
                kind: "view".to_string(),
                label: view.name,
                database: relation.database,
                schema: relation.schema,
                name: relation.name,
                metadata: Some(column_count_label(columns.len())),
                columns,
            })
        })
        .collect::<Result<_, CassieError>>()?;
    items.retain(|item| item.database.eq_ignore_ascii_case(database));

    items.sort_by_key(|item| item.label.to_ascii_lowercase());
    Ok(items)
}

fn index_items(cassie: &Cassie, database: &str) -> Result<Vec<QuerySchemaItem>, CassieError> {
    let mut items = Vec::new();
    for collection in cassie.catalog.list_collections_canonical() {
        for index in cassie.catalog.list_indexes(collection.name.as_str()) {
            let relation = canonical_relation(&index.collection)?;
            let fields = if index.normalized_fields().is_empty() {
                index.normalized_expressions().join(", ")
            } else {
                index.normalized_fields().join(", ")
            };
            items.push(QuerySchemaItem {
                id: format!("index:{}:{}", index.collection, index.name),
                kind: "index".to_string(),
                label: index.name.clone(),
                database: relation.database,
                schema: relation.schema,
                name: index.name.clone(),
                columns: Vec::new(),
                metadata: Some(format!(
                    "{} on {}({})",
                    index_kind_label(&index.kind),
                    index.collection,
                    fields
                )),
            });
        }
    }
    items.retain(|item| item.database.eq_ignore_ascii_case(database));
    items.sort_by_key(|item| item.label.to_ascii_lowercase());
    Ok(items)
}

fn function_items(cassie: &Cassie, database: &str) -> Result<Vec<QuerySchemaItem>, CassieError> {
    let mut items: Vec<QuerySchemaItem> = cassie
        .catalog
        .list_functions()
        .into_iter()
        .map(|function| {
            let relation = canonical_relation(&function.name)?;
            let args = function
                .args
                .iter()
                .map(|arg| format!("{} {}", arg.name, arg.data_type.type_name()))
                .collect::<Vec<_>>()
                .join(", ");
            Ok(QuerySchemaItem {
                id: format!("udf:{}", function.name),
                kind: "udf".to_string(),
                label: function.name,
                database: relation.database,
                schema: relation.schema,
                name: relation.name,
                columns: Vec::new(),
                metadata: Some(format!("({args}) -> {}", function.return_type.type_name())),
            })
        })
        .collect::<Result<_, CassieError>>()?;

    items.retain(|item| item.database.eq_ignore_ascii_case(database));
    items.sort_by_key(|item| item.label.to_ascii_lowercase());
    Ok(items)
}

fn procedure_items(cassie: &Cassie, database: &str) -> Result<Vec<QuerySchemaItem>, CassieError> {
    let mut items: Vec<QuerySchemaItem> = cassie
        .catalog
        .list_procedures()
        .into_iter()
        .map(|procedure| {
            let relation = canonical_relation(&procedure.name)?;
            let args = procedure
                .args
                .iter()
                .map(|arg| format!("{} {}", arg.name, arg.data_type.type_name()))
                .collect::<Vec<_>>()
                .join(", ");
            Ok(QuerySchemaItem {
                id: format!("procedure:{}", procedure.name),
                kind: "procedure".to_string(),
                label: procedure.name,
                database: relation.database,
                schema: relation.schema,
                name: relation.name,
                columns: Vec::new(),
                metadata: Some(format!("({args})")),
            })
        })
        .collect::<Result<_, CassieError>>()?;

    items.retain(|item| item.database.eq_ignore_ascii_case(database));
    items.sort_by_key(|item| item.label.to_ascii_lowercase());
    Ok(items)
}

fn canonical_relation(name: &str) -> Result<RelationId, CassieError> {
    RelationId::parse_canonical(name).ok_or_else(|| {
        CassieError::InvalidQuery(format!("catalog object name is not canonical: {name}"))
    })
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
        QueryStatement::GrantDatabaseConnect(_) => "GRANT",
        QueryStatement::RevokeDatabaseConnect(_) => "REVOKE",
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

impl TryFrom<QueryResult> for RestQueryResult {
    type Error = CassieError;

    fn try_from(result: QueryResult) -> Result<Self, Self::Error> {
        let rows = result
            .rows
            .into_iter()
            .map(|row| {
                row.into_iter()
                    .map(try_value_to_json)
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(rest_json_conversion_error)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            columns: result
                .columns
                .into_iter()
                .map(RestColumnMeta::from)
                .collect(),
            rows,
            command: result.command,
        })
    }
}

impl TryFrom<QueryExplainOutput> for RestQueryExplainResponse {
    type Error = CassieError;

    fn try_from(output: QueryExplainOutput) -> Result<Self, Self::Error> {
        let result = RestQueryResult::try_from(output.result)?;
        Ok(Self {
            columns: result.columns,
            rows: result.rows,
            command: result.command,
            plan: output.plan,
        })
    }
}

fn rest_json_conversion_error(error: &'static str) -> CassieError {
    CassieError::Execution(format!("REST query result {error}"))
}

#[cfg(test)]
mod tests {
    use super::{QueryResult, RestQueryResult};
    use crate::types::Value;

    #[test]
    fn should_bound_rest_json_row_storage_by_reserved_shape() {
        // Arrange
        let shapes = [(1, 2), (3, 3), (8193, 2), (1, 8193)];

        // Act
        let outputs = shapes.map(|(row_count, column_count)| {
            RestQueryResult::try_from(QueryResult {
                columns: Vec::new(),
                rows: vec![vec![Value::Int64(1); column_count]; row_count],
                command: "SELECT".to_owned(),
            })
            .expect("finite output")
        });

        // Assert
        for (output, (row_count, column_count)) in outputs.iter().zip(shapes) {
            assert_eq!(output.rows.len(), row_count);
            assert!(
                output.rows.capacity() <= row_count,
                "outer row capacity exceeds the query reservation"
            );
            for row in &output.rows {
                assert_eq!(row.len(), column_count);
                assert!(
                    row.capacity() <= column_count,
                    "JSON column capacity exceeds the query reservation"
                );
            }
        }
    }
}
