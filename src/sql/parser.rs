use crate::catalog::{ConstraintCheck, ConstraintOperator, FieldConstraint, IndexKind};
use crate::sql::ast::{
    AlterRoleStatement, AlterSchemaOperation, AlterSchemaStatement, AlterTableOperation,
    AlterTableStatement, BinaryOp, CallProcedureStatement, CommonTableExpression,
    CreateFunctionStatement, CreateIndexStatement, CreateProcedureStatement, CreateRoleStatement,
    CreateSchemaStatement, CreateTableStatement, CreateViewStatement, CteQuery,
    DropFunctionStatement, DropIndexStatement, DropProcedureStatement, DropRoleStatement,
    DropSchemaStatement, DropTableStatement, DropViewStatement, ExplainStatement, Expr,
    FieldDefinition, FunctionArg, FunctionCall, InsertSource, JoinKind, NullsOrder, OrderExpr,
    ParsedStatement, QuerySource, QueryStatement, SelectItem, SelectSet, SelectStatement,
    SetOperator, SetStatement, ShowStatement, SortDirection, TransactionAction,
    TransactionIsolation, TransactionStatement, Volatility, WindowFunctionCall,
};
use crate::types::DataType;
use serde_json::Value;
use std::collections::HashSet;

#[derive(Debug)]
pub struct SqlError(pub String);

#[path = "parser/clauses.rs"]
mod clauses;
#[path = "parser/dml.rs"]
mod dml;
#[path = "parser/expr.rs"]
mod expr;
#[path = "parser/query.rs"]
mod query;
#[path = "parser/schema.rs"]
mod schema;
#[path = "parser/statements.rs"]
mod statements;

use clauses::*;
use dml::*;
pub(crate) use expr::parse_expression;
use query::*;
use schema::*;
use statements::*;

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
    } else if starts_statement(lower, "drop view") {
        Ok(Some(parse_drop_view_statement(trimmed)?))
    } else if starts_statement(lower, "alter view") {
        Err(SqlError(
            "ALTER VIEW is not supported in this version".into(),
        ))
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
    } else if starts_statement(lower, "drop table") {
        Ok(Some(parse_drop_table_statement(trimmed)?))
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
