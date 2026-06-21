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
    if lower.starts_with("explain ") || lower == "explain" {
        parse_explain_statement(trimmed)
    } else if lower.starts_with("with ") || lower == "with" {
        parse_with_statement(trimmed)
    } else if lower.starts_with("insert ") || lower == "insert" {
        parse_insert_statement(trimmed)
    } else if lower.starts_with("update ") || lower == "update" {
        parse_update_statement(trimmed)
    } else if lower.starts_with("delete ") || lower == "delete" {
        parse_delete_statement(trimmed)
    } else if is_transaction_control_statement(&lower) {
        parse_transaction_statement(trimmed)
    } else if is_unsupported_transaction_control_statement(&lower) {
        Err(SqlError("unsupported transaction control statement".into()))
    } else if lower.starts_with("create role ") || lower == "create role" {
        parse_create_role_statement(trimmed, false)
    } else if lower.starts_with("create user ") || lower == "create user" {
        parse_create_role_statement(trimmed, true)
    } else if lower.starts_with("alter role ") || lower == "alter role" {
        parse_alter_role_statement(trimmed, false)
    } else if lower.starts_with("alter user ") || lower == "alter user" {
        parse_alter_role_statement(trimmed, true)
    } else if lower.starts_with("drop role ")
        || lower == "drop role"
        || lower.starts_with("drop user ")
        || lower == "drop user"
    {
        parse_drop_role_statement(trimmed)
    } else if let Some(message) = unsupported_privilege_statement(&lower) {
        Err(SqlError(message.to_string()))
    } else if lower.starts_with("create function ") || lower == "create function" {
        parse_create_function_statement(trimmed)
    } else if lower.starts_with("create procedure ") || lower == "create procedure" {
        parse_create_procedure_statement(trimmed)
    } else if lower.starts_with("create view ") || lower == "create view" {
        parse_create_view_statement(trimmed)
    } else if lower.starts_with("select ") {
        parse_select_statement(trimmed, Vec::new(), false)
    } else if lower.starts_with("create unique index ")
        || lower.starts_with("create index ")
        || lower == "create index"
    {
        parse_create_index_statement(trimmed)
    } else if lower.starts_with("drop index ") || lower == "drop index" {
        parse_drop_index_statement(trimmed)
    } else if lower.starts_with("drop function ") || lower == "drop function" {
        parse_drop_function_statement(trimmed)
    } else if lower.starts_with("drop procedure ") || lower == "drop procedure" {
        parse_drop_procedure_statement(trimmed)
    } else if lower.starts_with("drop view ") || lower == "drop view" {
        parse_drop_view_statement(trimmed)
    } else if lower.starts_with("call ") {
        parse_call_statement(trimmed)
    } else if lower.starts_with("create table ") || lower == "create table" {
        parse_create_table_statement(trimmed)
    } else if lower.starts_with("drop table ") || lower == "drop table" {
        parse_drop_table_statement(trimmed)
    } else if lower.starts_with("drop schema ") || lower == "drop schema" {
        parse_drop_schema_statement(trimmed)
    } else if lower.starts_with("alter table ") || lower == "alter table" {
        parse_alter_table_statement(trimmed)
    } else if lower.starts_with("alter schema ") || lower == "alter schema" {
        parse_alter_schema_statement(trimmed)
    } else if lower.starts_with("alter view ") || lower == "alter view" {
        Err(SqlError(
            "ALTER VIEW is not supported in this version".into(),
        ))
    } else if lower.starts_with("create schema ") || lower == "create schema" {
        parse_create_schema_statement(trimmed)
    } else if lower.starts_with("show ") || lower == "show" {
        parse_show_statement(trimmed)
    } else if lower.starts_with("set ") || lower == "set" {
        parse_set_statement(trimmed)
    } else {
        Err(SqlError("unsupported SQL statement".into()))
    }
}
