//! Initial binding with source scopes, before parameter-aware rewrites and planning.
use super::{
    alias_admission, bind_boolean_parameters, bind_generated_source_inputs, bind_statement,
    bound_indexes, conditional_bindings, subquery_types, whole_row, BindingContext, BoundStatement,
    CassieError, Catalog, CteScope, HashMap, ParsedStatement,
};

/// # Errors
///
/// Returns an error when validation, storage, or execution fails.
pub fn bind(statement: ParsedStatement, catalog: &Catalog) -> Result<BoundStatement, CassieError> {
    bind_with_context(statement, catalog, &BindingContext::default())
}

/// # Errors
///
/// Returns an error when validation, storage, or execution fails.
pub fn bind_with_context(
    statement: ParsedStatement,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<BoundStatement, CassieError> {
    bind_initial_with_controls(statement, catalog, context, None)
}

pub(crate) fn bind_initial_with_controls(
    statement: ParsedStatement,
    catalog: &Catalog,
    context: &BindingContext,
    controls: Option<&crate::runtime::QueryExecutionControls>,
) -> Result<BoundStatement, CassieError> {
    bind_initial_with_declared_types(statement, catalog, context, controls, &[])
}

pub(crate) fn bind_initial_with_declared_types(
    statement: ParsedStatement,
    catalog: &Catalog,
    context: &BindingContext,
    controls: Option<&crate::runtime::QueryExecutionControls>,
    declared_oids: &[i32],
) -> Result<BoundStatement, CassieError> {
    alias_admission::validate(&statement, catalog, context, controls)?;
    let mut statement = statement;
    whole_row::rewrite(&mut statement, catalog, context)?;
    subquery_types::annotate_unbound(&mut statement, catalog, context);
    let scope = CteScope::new(controls, declared_oids);
    let mut statement = bind_statement(statement, catalog, &scope, context)?;
    subquery_types::annotate(&mut statement, catalog, context)?;
    if crate::sql::parameter_count(&statement) == 0 {
        // Parameter-free view/projection callers also consume this bound AST.
        // Parameterized statements wait for their concrete OID-aware pass.
        bind_boolean_parameters(&mut statement, catalog, context, &[])?;
        bind_generated_source_inputs(&mut statement, catalog, context, &[])?;
    }
    conditional_bindings::coerce_statement(&mut statement, catalog, context, &HashMap::new())?;
    let indexes = bound_indexes(&statement, catalog)?;
    Ok(BoundStatement { statement, indexes })
}
