//! Engine compatibility facade for the syntax crate's parser.

use crate::sql::ast::{Expr, ParsedStatement};

pub use cassie_sql::parser::{
    dollar_quoted_content, dollar_quoted_end, escape_string_prefix,
    is_concurrent_materialized_view_refresh, max_positional_parameter, parse_data_type_in_scope,
    parse_terminal_transaction_control, preflight_sql, substitute_positional_parameters,
    with_domain_scope, with_enum_scope, with_type_scope, DomainResolver, EnumResolver,
    SqlError, SqlErrorKind, TypeResolver,
};

/// Parses SQL syntax and rebinds its runtime-only subquery attachment type.
pub fn parse_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let parsed = cassie_sql::parse_statement(sql)?;
    Ok(cassie_sql::attachment::rebind_attachment(parsed))
}

/// Parses one expression and rebinds its runtime-only subquery attachment type.
pub fn parse_expression(sql: &str) -> Result<Expr, SqlError> {
    let expression = cassie_sql::parser::parse_expression(sql)?;
    Ok(cassie_sql::attachment::rebind_attachment(expression))
}

#[cfg(test)]
#[path = "parser/expr_recursion_tests.rs"]
mod expr_recursion_tests;
