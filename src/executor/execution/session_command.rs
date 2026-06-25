use super::*;

pub(super) fn execute_show(
    statement: &crate::sql::ast::ShowStatement,
) -> Result<QueryResult, QueryError> {
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
        "transaction isolation level" => Ok(QueryResult {
            columns: vec![ColumnMeta::text("transaction_isolation")],
            rows: vec![vec![Value::String("read committed".to_string())]],
            command: "SHOW".to_string(),
        }),
        "standard_conforming_strings" => Ok(QueryResult {
            columns: vec![ColumnMeta::text("standard_conforming_strings")],
            rows: vec![vec![Value::String("on".to_string())]],
            command: "SHOW".to_string(),
        }),
        _ => Err(QueryError::General(format!(
            "unsupported SHOW variable '{}'",
            statement.variable
        ))),
    }
}

pub(super) fn execute_set(
    statement: &crate::sql::ast::SetStatement,
) -> Result<QueryResult, QueryError> {
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
