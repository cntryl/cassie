use crate::catalog::{ConstraintCheck, ConstraintOperator, FieldConstraint, IndexKind};
use crate::sql::ast::{
    AlterRetentionPolicyStatement, AlterRoleStatement, AlterSchemaOperation, AlterSchemaStatement,
    AlterTableOperation, AlterTableStatement, BinaryOp, CallProcedureStatement,
    CommonTableExpression, CreateFunctionStatement, CreateGraphStatement, CreateIndexStatement,
    CreateMaterializedProjectionStatement, CreateProcedureStatement,
    CreateRetentionPolicyStatement, CreateRoleStatement, CreateRollupStatement,
    CreateSchemaStatement, CreateSequenceStatement, CreateTableStatement, CreateViewStatement,
    CteQuery, DropFunctionStatement, DropIndexStatement, DropMaterializedProjectionStatement,
    DropProcedureStatement, DropRetentionPolicyStatement, DropRoleStatement, DropRollupStatement,
    DropSchemaStatement, DropSequenceStatement, DropTableStatement, DropViewStatement,
    EnforceRetentionPolicyStatement, ExplainStatement, Expr, FieldDefinition, FunctionArg,
    FunctionCall, InsertSource, JoinKind, NullsOrder, OrderExpr, ParsedStatement, QuerySource,
    QueryStatement, RefreshRollupStatement, SelectItem, SetStatement, ShowStatement, SortDirection,
    TransactionAction, TransactionIsolation, TransactionStatement, VerifyProjectionStatement,
    Volatility, WindowFunctionCall,
};
use crate::types::DataType;
use serde_json::Value;
use std::collections::HashSet;

#[derive(Debug)]
pub struct SqlError(pub String);

#[path = "parser/clauses.rs"]
mod clauses;
#[path = "parser/copy.rs"]
mod copy;
#[path = "parser/dml.rs"]
mod dml;
#[path = "parser/expr.rs"]
mod expr;
#[path = "parser/materialized_projection.rs"]
mod materialized_projection;
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
    parse_alter_role_statement, parse_alter_schema_statement, parse_alter_table_statement,
    parse_create_graph_statement, parse_create_index_statement, parse_create_role_statement,
    parse_create_schema_statement, parse_create_sequence_statement, parse_create_table_statement,
    parse_drop_index_statement, parse_drop_role_statement, parse_drop_schema_statement,
    parse_drop_sequence_statement, parse_drop_table_statement, parse_index_options,
};
use statements::{
    is_transaction_control_statement, is_unsupported_transaction_control_statement,
    parse_call_statement, parse_create_function_statement, parse_create_procedure_statement,
    parse_create_view_statement, parse_drop_function_statement, parse_drop_procedure_statement,
    parse_drop_view_statement, parse_explain_statement, parse_optional_role_password,
    parse_set_statement, parse_show_statement, parse_transaction_statement, split_keyword,
    unsupported_privilege_statement,
};

/// # Errors
///
/// Returns an error when validation, storage, or execution fails.
pub fn parse_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let lower = trimmed.to_lowercase();

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

    Err(SqlError("unsupported SQL statement".into()))
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
    } else if lower.starts_with("select ") {
        Ok(Some(parse_select_statement(trimmed, Vec::new(), false)?))
    } else if is_transaction_control_statement(lower) {
        Ok(Some(parse_transaction_statement(trimmed)?))
    } else if is_unsupported_transaction_control_statement(lower) {
        Err(SqlError("unsupported transaction control statement".into()))
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
    } else if let Some(message) = unsupported_privilege_statement(lower) {
        Err(SqlError(message.to_string()))
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
    if let Some(parsed) = parse_view_or_index_statement(trimmed, lower)? {
        return Ok(Some(parsed));
    }
    parse_table_or_schema_statement(trimmed, lower)
}

fn parse_view_or_index_statement(
    trimmed: &str,
    lower: &str,
) -> Result<Option<ParsedStatement>, SqlError> {
    if starts_statement(lower, "create view") {
        Ok(Some(parse_create_view_statement(trimmed)?))
    } else if starts_statement(lower, "create materialized projection") {
        Ok(Some(parse_create_materialized_projection_statement(
            trimmed,
        )?))
    } else if starts_statement(lower, "drop view") {
        Ok(Some(parse_drop_view_statement(trimmed)?))
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
        Err(SqlError(
            "ALTER VIEW is not supported in this version".into(),
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
    } else if starts_statement(lower, "create sequence") {
        Ok(Some(parse_create_sequence_statement(trimmed)?))
    } else if starts_statement(lower, "drop table") {
        Ok(Some(parse_drop_table_statement(trimmed)?))
    } else if starts_statement(lower, "drop sequence") {
        Ok(Some(parse_drop_sequence_statement(trimmed)?))
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
    } else {
        Ok(None)
    }
}

fn starts_statement(lower: &str, keyword: &str) -> bool {
    lower == keyword
        || lower
            .strip_prefix(keyword)
            .is_some_and(|remainder| remainder.starts_with(' '))
}
