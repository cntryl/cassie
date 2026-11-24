//! Engine diagnostics for the persisted syntax contract.

use super::ast::{CreateRollupStatement, Expr, ParsedStatement};
use crate::app::CassieError;
use crate::catalog::{
    FieldConstraint, IndexMeta, MaterializedProjectionMeta, RollupMeta, ViewMeta,
};

use cassie_sql::definition_guard as syntax;
pub(crate) use syntax::{is_legacy, null_safe_default};

impl From<syntax::DefinitionError> for CassieError {
    fn from(error: syntax::DefinitionError) -> Self {
        match error {
            syntax::DefinitionError::Unsupported(message) => Self::Unsupported(message),
            syntax::DefinitionError::Execution(message) => Self::Execution(message),
        }
    }
}

pub(crate) fn version(version: u32) -> Result<(), CassieError> {
    Ok(syntax::version(version)?)
}

pub(crate) fn expression(expr: &Expr) -> Result<(), CassieError> {
    Ok(syntax::expression(expr)?)
}

pub(crate) fn expression_sql(sql: &str) -> Result<(), CassieError> {
    Ok(syntax::expression_sql(sql)?)
}

pub(crate) fn query_sql(sql: &str) -> Result<(), CassieError> {
    Ok(syntax::query_sql(sql)?)
}

pub(crate) fn statement(query: &ParsedStatement) -> Result<(), CassieError> {
    Ok(syntax::statement(query)?)
}

pub(crate) fn create_rollup(definition: &CreateRollupStatement) -> Result<(), CassieError> {
    Ok(syntax::create_rollup(definition)?)
}

pub(crate) fn legacy_expression(expr: &Expr) -> Result<(), CassieError> {
    Ok(syntax::legacy_expression(expr)?)
}

pub(crate) fn legacy_expression_sql(sql: &str) -> Result<(), CassieError> {
    Ok(syntax::legacy_expression_sql(sql)?)
}

pub(crate) fn legacy_query_sql(sql: &str) -> Result<(), CassieError> {
    Ok(syntax::legacy_query_sql(sql)?)
}

pub(crate) fn legacy_statement(query: &ParsedStatement) -> Result<(), CassieError> {
    Ok(syntax::legacy_statement(query)?)
}

pub(crate) fn constraints(constraints: &[FieldConstraint]) -> Result<(), CassieError> {
    Ok(syntax::constraints(constraints)?)
}

pub(crate) fn index(metadata: &IndexMeta) -> Result<(), CassieError> {
    Ok(syntax::index(
        metadata.definition_semantics_version,
        &metadata.expressions,
        metadata.predicate.as_deref(),
    )?)
}

pub(crate) fn view(metadata: &ViewMeta) -> Result<(), CassieError> {
    Ok(syntax::query(
        metadata.definition_semantics_version,
        &metadata.query,
    )?)
}

pub(crate) fn projection(metadata: &MaterializedProjectionMeta) -> Result<(), CassieError> {
    Ok(syntax::query(
        metadata.definition_semantics_version,
        &metadata.query,
    )?)
}

pub(crate) fn rollup(metadata: &RollupMeta) -> Result<(), CassieError> {
    let expressions = metadata
        .group_keys
        .iter()
        .chain(
            metadata
                .aggregates
                .iter()
                .map(|aggregate| &aggregate.expression),
        )
        .chain(&metadata.filter_expr)
        .map(String::as_str);
    Ok(syntax::rollup(
        metadata.definition_semantics_version,
        &metadata.bucket_expr,
        expressions,
    )?)
}

#[cfg(test)]
pub(crate) fn encoded_expression(sql: &str) -> Result<(), CassieError> {
    Ok(syntax::encoded_expression(sql)?)
}

#[cfg(test)]
mod tests {
    #[test]
    fn should_reject_scalar_aggregate_modifiers_given_a_persisted_expression_when_definition_admission_runs(
    ) {
        // Arrange
        let cases = ["LOWER(label) FILTER (WHERE TRUE)", "LOWER(DISTINCT label)"];

        // Act
        for sql in cases {
            let expression =
                crate::sql::parser::parse_expression(sql).expect("function clause syntax");
            let error = super::expression(&expression)
                .expect_err("scalar modifiers cannot enter persisted definitions");

            // Assert
            assert_eq!(error.descriptor().sql_state, "42809");
        }
        let control =
            crate::sql::parser::parse_expression("LOWER(ALL label)").expect("valid ALL control");
        assert!(super::expression(&control).is_ok());
    }
}

#[cfg(test)]
#[path = "definition_guard_window_tests.rs"]
mod window_tests;
