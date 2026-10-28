use std::fmt;

#[path = "parser/aggregate_call.rs"]
mod aggregate_call;
#[path = "parser/pattern_function_call.rs"]
mod pattern_function_call;

use crate::catalog::{ConstraintCheck, ConstraintOperator, FieldConstraint, IndexKind};
use crate::sql::ast::{
    AlterIndexOperation, AlterIndexStatement, AlterRetentionPolicyStatement, AlterRoleStatement,
    AlterSchemaOperation, AlterSchemaStatement, AlterSequenceStatement, AlterTableOperation,
    AlterTableStatement, BinaryOp, CallProcedureStatement, CommonTableExpression,
    CreateDatabaseStatement, CreateFunctionStatement, CreateGraphStatement, CreateIndexStatement,
    CreateMaterializedProjectionStatement, CreateProcedureStatement,
    CreateRetentionPolicyStatement, CreateRoleStatement, CreateRollupStatement,
    CreateSchemaStatement, CreateSequenceStatement, CreateTableStatement, CreateViewStatement,
    DatabaseConnectPrivilegeStatement, DropDatabaseStatement, DropFunctionStatement,
    DropIndexStatement, DropMaterializedProjectionStatement, DropProcedureStatement,
    DropRetentionPolicyStatement, DropRoleStatement, DropRollupStatement, DropSchemaStatement,
    DropSequenceStatement, DropTableStatement, DropViewStatement, EnforceRetentionPolicyStatement,
    ExplainStatement, Expr, FieldDefinition, FunctionArg, FunctionCall, InsertSource, JoinKind,
    NullsOrder, OrderExpr, ParsedStatement, QuerySource, QueryStatement, RefreshRollupStatement,
    SelectItem, SortDirection, VerifyProjectionStatement, Volatility, WindowFunctionCall,
};
use crate::types::DataType;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlErrorKind {
    Syntax,
    Unsupported,
    NumericRange,
    DuplicateColumn,
    ResourceLimit,
    UndefinedFunction,
    UndefinedObject,
    /// A type modifier outside its documented range (SQLSTATE 22023).
    InvalidParameterValue,
}

#[derive(Debug, Clone)]
pub struct SqlError {
    kind: SqlErrorKind,
    message: String,
}

impl SqlError {
    #[must_use]
    pub fn new(message: String) -> Self {
        let kind = infer_sql_error_kind(&message);
        Self { kind, message }
    }

    #[must_use]
    pub fn syntax(message: String) -> Self {
        Self {
            kind: SqlErrorKind::Syntax,
            message,
        }
    }

    #[must_use]
    pub fn unsupported(message: String) -> Self {
        Self {
            kind: SqlErrorKind::Unsupported,
            message,
        }
    }

    #[must_use]
    pub fn undefined_function(message: String) -> Self {
        Self {
            kind: SqlErrorKind::UndefinedFunction,
            message,
        }
    }

    #[must_use]
    pub fn undefined_object(message: String) -> Self {
        Self {
            kind: SqlErrorKind::UndefinedObject,
            message,
        }
    }

    #[must_use]
    pub fn invalid_parameter_value(message: String) -> Self {
        Self {
            kind: SqlErrorKind::InvalidParameterValue,
            message,
        }
    }

    #[must_use]
    pub fn numeric_range(message: String) -> Self {
        Self {
            kind: SqlErrorKind::NumericRange,
            message,
        }
    }

    pub(super) fn duplicate_column(name: &str) -> Self {
        Self {
            kind: SqlErrorKind::DuplicateColumn,
            message: format!("column \"{name}\" specified more than once"),
        }
    }

    #[must_use]
    pub fn resource_limit(message: String) -> Self {
        Self {
            kind: SqlErrorKind::ResourceLimit,
            message,
        }
    }

    #[must_use]
    pub const fn kind(&self) -> SqlErrorKind {
        self.kind
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for SqlError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for SqlError {}

fn infer_sql_error_kind(message: &str) -> SqlErrorKind {
    let lower = message.to_ascii_lowercase();
    if lower.contains("unsupported") || lower.contains("not supported") {
        SqlErrorKind::Unsupported
    } else {
        SqlErrorKind::Syntax
    }
}

#[path = "parser/clauses.rs"]
mod clauses;
#[path = "parser/copy.rs"]
mod copy;
#[path = "parser/dml.rs"]
mod dml;
#[path = "parser/dml_conflict.rs"]
mod dml_conflict;
#[path = "parser/dml_targets.rs"]
mod dml_targets;
#[path = "parser/expr.rs"]
mod expr;
#[path = "parser/identifiers.rs"]
mod identifiers;
mod lexical;
#[path = "parser/merge.rs"]
mod merge;
#[path = "parser/truncate.rs"]
mod truncate;
#[path = "parser/with_statement.rs"]
mod with_statement;
pub(crate) use lexical::{dollar_quoted_content, dollar_quoted_end, escape_string_prefix};
#[path = "parser/materialized_projection.rs"]
mod materialized_projection;
#[path = "parser/materialized_view.rs"]
mod materialized_view;
#[path = "parser/operators.rs"]
mod operators;
#[path = "parser/order_ordinals.rs"]
mod order_ordinals;
#[path = "parser/parameter_text.rs"]
mod parameter_text;
pub(crate) use parameter_text::{max_positional_parameter, substitute_positional_parameters};
#[path = "parser/preflight.rs"]
mod preflight;
#[path = "parser/query.rs"]
mod query;
#[path = "parser/retention.rs"]
mod retention;
#[path = "parser/rollups.rs"]
mod rollups;
#[path = "parser/schema.rs"]
mod schema;
#[path = "parser/statements.rs"]
mod statements;

use clauses::{find_top_level_keyword, strip_parentheses};
use copy::parse_copy_statement;
use dml::{
    find_matching_paren, parse_delete_statement, parse_insert_statement, parse_update_statement,
};
pub(crate) use expr::parse_expression;
use materialized_projection::{
    parse_alter_materialized_projection_statement, parse_compare_projection_statement,
    parse_create_materialized_projection_statement, parse_diff_projection_statement,
    parse_drop_materialized_projection_statement,
    parse_drop_materialized_projection_version_statement, parse_plan_repair_projection_statement,
    parse_refresh_materialized_projection_statement, parse_repair_projection_statement,
    parse_verify_projection_statement,
};
use materialized_view::{
    parse_create_materialized_view_statement, parse_drop_materialized_view_statement,
    parse_refresh_materialized_view_statement,
};
pub(crate) use preflight::preflight_sql;
use query::{
    parse_enclosed_parenthesized, parse_projection_items, parse_select_statement,
    parse_with_statement,
};
use retention::{
    parse_alter_retention_policy_statement, parse_create_retention_policy_statement,
    parse_drop_retention_policy_statement, parse_enforce_retention_policy_statement,
};
use rollups::{
    parse_create_rollup_statement, parse_drop_rollup_statement, parse_refresh_rollup_statement,
};
use schema::{
    parse_alter_index_statement, parse_alter_role_statement, parse_alter_schema_statement,
    parse_alter_sequence_statement, parse_alter_table_statement, parse_create_database_statement,
    parse_create_graph_statement, parse_create_index_statement, parse_create_role_statement,
    parse_create_schema_statement, parse_create_sequence_statement, parse_create_table_statement,
    parse_database_connect_privilege_statement, parse_drop_database_statement,
    parse_drop_index_statement, parse_drop_role_statement, parse_drop_schema_statement,
    parse_drop_sequence_statement, parse_drop_table_statement, parse_index_options,
    parse_reindex_statement,
};
use statements::{
    is_transaction_control_statement, is_unsupported_transaction_control_statement,
    parse_call_statement, parse_create_function_statement, parse_create_procedure_statement,
    parse_create_view_statement, parse_drop_function_statement, parse_drop_procedure_statement,
    parse_drop_view_statement, parse_explain_statement, parse_optional_role_password,
    parse_reset_statement, parse_set_statement, parse_show_statement, parse_transaction_statement,
    split_keyword, unsupported_privilege_statement,
};

/// # Errors
///
/// Returns an error when validation, storage, or execution fails.
pub fn parse_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    preflight_sql(sql)?;
    let raw_sql = sql.trim().trim_end_matches(';').trim();
    let decoded = lexical::decode_unicode_identifiers(raw_sql)?;
    let start = lexical::separator_end(&decoded, 0);
    let parsed = parse_statement_body(&decoded[start..])?;
    Ok(ParsedStatement {
        raw_sql: raw_sql.to_string(),
        statement: parsed.statement,
    })
}

pub(crate) fn is_concurrent_materialized_view_refresh(sql: &str) -> bool {
    materialized_view::is_concurrent_refresh(sql)
}

fn parse_statement_body(sql: &str) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let lower = trimmed.to_ascii_lowercase();

    if let Some(parsed) = parse_query_or_dml_statement(trimmed, &lower)? {
        return Ok(parsed);
    }
    if let Some(parsed) = parse_access_control_statement(trimmed, &lower)? {
        return Ok(parsed);
    }
    if let Some(parsed) = parse_routine_statement(trimmed, &lower)? {
        return Ok(parsed);
    }
    if let Some(parsed) = parse_schema_statement(trimmed, &lower)? {
        return Ok(parsed);
    }
    if let Some(parsed) = parse_session_statement(trimmed, &lower)? {
        return Ok(parsed);
    }

    Err(SqlError::unsupported(
        "unsupported SQL statement".to_string(),
    ))
}

fn parse_query_or_dml_statement(
    trimmed: &str,
    lower: &str,
) -> Result<Option<ParsedStatement>, SqlError> {
    if starts_statement(lower, "explain") {
        Ok(Some(parse_explain_statement(trimmed)?))
    } else if starts_statement(lower, "with") {
        Ok(Some(parse_with_statement(trimmed)?))
    } else if starts_statement(lower, "copy") {
        Ok(Some(parse_copy_statement(trimmed)?))
    } else if starts_statement(lower, "insert") {
        Ok(Some(parse_insert_statement(trimmed)?))
    } else if starts_statement(lower, "update") {
        Ok(Some(parse_update_statement(trimmed)?))
    } else if starts_statement(lower, "delete") {
        Ok(Some(parse_delete_statement(trimmed)?))
    } else if starts_statement(lower, "truncate") {
        Ok(Some(truncate::parse_truncate_statement(trimmed)?))
    } else if starts_statement(lower, "merge") {
        Ok(Some(merge::parse_merge_statement(trimmed, Vec::new())?))
    } else if starts_statement(lower, "table") {
        // `TABLE name` is `SELECT * FROM name`.
        let select = format!("SELECT * FROM {}", &trimmed["table".len()..]);
        let mut parsed = parse_select_statement(&select, Vec::new(), false)?;
        trimmed.clone_into(&mut parsed.raw_sql);
        Ok(Some(parsed))
    } else if lexical::pattern_end(lower, 0, "select").is_some()
        || lexical::pattern_end(lower, 0, "values").is_some()
        || trimmed.starts_with('(')
    {
        Ok(Some(parse_select_statement(trimmed, Vec::new(), false)?))
    } else if is_transaction_control_statement(lower) {
        Ok(Some(parse_transaction_statement(trimmed)?))
    } else if is_unsupported_transaction_control_statement(lower) {
        Err(SqlError::unsupported(
            "unsupported transaction control statement".to_string(),
        ))
    } else {
        Ok(None)
    }
}

fn parse_access_control_statement(
    trimmed: &str,
    lower: &str,
) -> Result<Option<ParsedStatement>, SqlError> {
    if starts_statement(lower, "create role") {
        Ok(Some(parse_create_role_statement(trimmed, false)?))
    } else if starts_statement(lower, "create user") {
        Ok(Some(parse_create_role_statement(trimmed, true)?))
    } else if starts_statement(lower, "alter role") {
        Ok(Some(parse_alter_role_statement(trimmed, false)?))
    } else if starts_statement(lower, "alter user") {
        Ok(Some(parse_alter_role_statement(trimmed, true)?))
    } else if starts_statement(lower, "drop role") || starts_statement(lower, "drop user") {
        Ok(Some(parse_drop_role_statement(trimmed)?))
    } else if starts_statement(lower, "grant connect on database") {
        Ok(Some(parse_database_connect_privilege_statement(
            trimmed, true,
        )?))
    } else if starts_statement(lower, "revoke connect on database") {
        Ok(Some(parse_database_connect_privilege_statement(
            trimmed, false,
        )?))
    } else if let Some(message) = unsupported_privilege_statement(lower) {
        Err(SqlError::unsupported(message.to_string()))
    } else {
        Ok(None)
    }
}

fn parse_routine_statement(
    trimmed: &str,
    lower: &str,
) -> Result<Option<ParsedStatement>, SqlError> {
    if starts_statement(lower, "create function") {
        Ok(Some(parse_create_function_statement(trimmed)?))
    } else if starts_statement(lower, "create procedure") {
        Ok(Some(parse_create_procedure_statement(trimmed)?))
    } else if starts_statement(lower, "drop function") {
        Ok(Some(parse_drop_function_statement(trimmed)?))
    } else if starts_statement(lower, "drop procedure") {
        Ok(Some(parse_drop_procedure_statement(trimmed)?))
    } else if lower.starts_with("call ") {
        Ok(Some(parse_call_statement(trimmed)?))
    } else {
        Ok(None)
    }
}

fn parse_schema_statement(trimmed: &str, lower: &str) -> Result<Option<ParsedStatement>, SqlError> {
    if starts_statement(lower, "create collation") {
        return Err(SqlError::unsupported(
            "CREATE COLLATION is not supported in this version".into(),
        ));
    }
    if starts_statement(lower, "alter collation") {
        return Err(SqlError::unsupported(
            "ALTER COLLATION is not supported in this version".into(),
        ));
    }
    if let Some(parsed) = parse_view_or_index_statement(trimmed, lower)? {
        return Ok(Some(parsed));
    }
    parse_table_or_schema_statement(trimmed, lower)
}

fn parse_view_or_index_statement(
    trimmed: &str,
    lower: &str,
) -> Result<Option<ParsedStatement>, SqlError> {
    if starts_statement(lower, "create materialized view") {
        Ok(Some(parse_create_materialized_view_statement(trimmed)?))
    } else if starts_statement(lower, "create view") {
        Ok(Some(parse_create_view_statement(trimmed)?))
    } else if starts_statement(lower, "create materialized projection") {
        Ok(Some(parse_create_materialized_projection_statement(
            trimmed,
        )?))
    } else if starts_statement(lower, "drop materialized view") {
        Ok(Some(parse_drop_materialized_view_statement(trimmed)?))
    } else if starts_statement(lower, "drop view") {
        Ok(Some(parse_drop_view_statement(trimmed)?))
    } else if starts_statement(lower, "refresh materialized view") {
        Ok(Some(parse_refresh_materialized_view_statement(trimmed)?))
    } else if starts_statement(lower, "refresh materialized projection") {
        Ok(Some(parse_refresh_materialized_projection_statement(
            trimmed,
        )?))
    } else if starts_statement(lower, "drop materialized projection version") {
        Ok(Some(parse_drop_materialized_projection_version_statement(
            trimmed,
        )?))
    } else if starts_statement(lower, "drop materialized projection") {
        Ok(Some(parse_drop_materialized_projection_statement(trimmed)?))
    } else if starts_statement(lower, "alter materialized projection") {
        Ok(Some(parse_alter_materialized_projection_statement(
            trimmed,
        )?))
    } else if starts_statement(lower, "verify projection") {
        Ok(Some(parse_verify_projection_statement(trimmed)?))
    } else if starts_statement(lower, "diff projection") {
        Ok(Some(parse_diff_projection_statement(trimmed)?))
    } else if starts_statement(lower, "compare projection") {
        Ok(Some(parse_compare_projection_statement(trimmed)?))
    } else if starts_statement(lower, "plan repair projection") {
        Ok(Some(parse_plan_repair_projection_statement(trimmed)?))
    } else if starts_statement(lower, "repair projection") {
        Ok(Some(parse_repair_projection_statement(trimmed)?))
    } else if starts_statement(lower, "alter view") {
        Err(SqlError::unsupported(
            "ALTER VIEW is not supported in this version".to_string(),
        ))
    } else if starts_statement(lower, "create rollup") {
        Ok(Some(parse_create_rollup_statement(trimmed)?))
    } else if starts_statement(lower, "refresh rollup") {
        Ok(Some(parse_refresh_rollup_statement(trimmed)?))
    } else if starts_statement(lower, "drop rollup") {
        Ok(Some(parse_drop_rollup_statement(trimmed)?))
    } else if starts_statement(lower, "create retention policy") {
        Ok(Some(parse_create_retention_policy_statement(trimmed)?))
    } else if starts_statement(lower, "alter retention policy") {
        Ok(Some(parse_alter_retention_policy_statement(trimmed)?))
    } else if starts_statement(lower, "drop retention policy") {
        Ok(Some(parse_drop_retention_policy_statement(trimmed)?))
    } else if starts_statement(lower, "enforce retention policy") {
        Ok(Some(parse_enforce_retention_policy_statement(trimmed)?))
    } else if starts_statement(lower, "create unique index")
        || starts_statement(lower, "create index")
    {
        Ok(Some(parse_create_index_statement(trimmed)?))
    } else if starts_statement(lower, "drop index") {
        Ok(Some(parse_drop_index_statement(trimmed)?))
    } else if starts_statement(lower, "alter index") {
        Ok(Some(parse_alter_index_statement(trimmed)?))
    } else if starts_statement(lower, "reindex") {
        Ok(Some(parse_reindex_statement(trimmed)?))
    } else {
        Ok(None)
    }
}

fn parse_table_or_schema_statement(
    trimmed: &str,
    lower: &str,
) -> Result<Option<ParsedStatement>, SqlError> {
    if starts_statement(lower, "create table") {
        Ok(Some(parse_create_table_statement(trimmed)?))
    } else if starts_statement(lower, "create graph") {
        Ok(Some(parse_create_graph_statement(trimmed)?))
    } else if starts_statement(lower, "create sequence")
        || starts_statement(lower, "create temporary sequence")
        || starts_statement(lower, "create temp sequence")
        || starts_statement(lower, "create unlogged sequence")
    {
        Ok(Some(parse_create_sequence_statement(trimmed)?))
    } else if starts_statement(lower, "alter sequence") {
        Ok(Some(parse_alter_sequence_statement(trimmed)?))
    } else if starts_statement(lower, "create database") {
        Ok(Some(parse_create_database_statement(trimmed)?))
    } else if starts_statement(lower, "drop table") {
        Ok(Some(parse_drop_table_statement(trimmed)?))
    } else if starts_statement(lower, "drop sequence") {
        Ok(Some(parse_drop_sequence_statement(trimmed)?))
    } else if starts_statement(lower, "drop database") {
        Ok(Some(parse_drop_database_statement(trimmed)?))
    } else if starts_statement(lower, "alter table") {
        Ok(Some(parse_alter_table_statement(trimmed)?))
    } else if starts_statement(lower, "create schema") {
        Ok(Some(parse_create_schema_statement(trimmed)?))
    } else if starts_statement(lower, "drop schema") {
        Ok(Some(parse_drop_schema_statement(trimmed)?))
    } else if starts_statement(lower, "alter schema") {
        Ok(Some(parse_alter_schema_statement(trimmed)?))
    } else {
        Ok(None)
    }
}

fn parse_session_statement(
    trimmed: &str,
    lower: &str,
) -> Result<Option<ParsedStatement>, SqlError> {
    if starts_statement(lower, "show") {
        Ok(Some(parse_show_statement(trimmed)?))
    } else if starts_statement(lower, "set") {
        Ok(Some(parse_set_statement(trimmed)?))
    } else if starts_statement(lower, "reset") {
        Ok(Some(parse_reset_statement(trimmed)?))
    } else if statements::SESSION_OBJECT_KEYWORDS
        .iter()
        .any(|keyword| starts_statement(lower, keyword))
    {
        Ok(Some(statements::parse_session_object_statement(trimmed)?))
    } else {
        Ok(None)
    }
}

fn starts_statement(lower: &str, keyword: &str) -> bool {
    lower == keyword
        || lower
            .strip_prefix(keyword)
            .is_some_and(|remainder| lexical::separator_end(remainder, 0) > 0)
}

#[cfg(test)]
#[path = "parser/cte_materialization_tests.rs"]
mod cte_materialization_tests;

#[path = "parser/cte_materialization.rs"]
mod cte_materialization;
