//! Correlated `EXISTS` in a `WHERE` filter.
//!
//! A subquery that references the enclosing row cannot be folded to one
//! boolean up front. Each candidate row is instead offered to the subquery as
//! its outer row (the same mechanism a `LATERAL` source uses): the binder sees
//! the row's columns as extra known fields and the executor merges the row
//! into every subquery source row, so `orders.uid = users.uid` compares the
//! inner column with this outer row's value.

use super::{
    batch, filter, resolve_exists_expr, Batch, BatchRow, ExistsResolutionContext, Expr, QueryError,
    QuerySource,
};

use super::LogicalPlan;

/// Returns whether `expr` contains an `EXISTS` subquery.
pub(super) fn contains_exists(expr: &Expr) -> bool {
    expr.any_descendant_or_self(&mut |expr| matches!(expr, Expr::Exists(_)))
}

/// Keeps the rows of `batches` for which `filter_expr` holds, evaluating each
/// `EXISTS` with that row as the subquery's outer row.
///
/// # Errors
///
/// Returns an error when a subquery fails to bind or execute.
pub(super) fn filter_rows_per_outer_row(
    context: &ExistsResolutionContext<'_>,
    source: &QuerySource,
    filter_expr: &Expr,
    batches: Vec<Batch>,
    search_context: Option<&filter::SearchContext>,
) -> Result<Vec<Batch>, QueryError> {
    let qualifier = outer_qualifier(source);
    let mut kept = Vec::new();
    for row in batch::flatten_batches(batches) {
        super::check_timeout(context.controls)?;
        let outer = qualified_outer_row(context, source, &row, qualifier.as_deref())?;
        let row_context = ExistsResolutionContext {
            outer_row: Some(&outer),
            ..*context
        };
        let resolved = resolve_exists_expr(&row_context, filter_expr)?;
        kept.extend(filter::filter_rows(
            vec![row],
            &resolved,
            context.params,
            search_context,
            context.user_functions,
            context.session,
        )?);
    }
    Ok(batch::chunk_rows(kept, batch::DEFAULT_BATCH_SIZE))
}

/// The name the enclosing relation's columns are qualified with, for a
/// single-relation source whose rows carry unqualified column names.
pub(super) fn outer_qualifier(source: &QuerySource) -> Option<String> {
    match source {
        QuerySource::Collection(name) => {
            Some(crate::catalog::local_name(name).to_ascii_lowercase())
        }
        QuerySource::Subquery { alias, .. } => Some(alias.to_ascii_lowercase()),
        QuerySource::Cte(name) | QuerySource::TableFunction { name, .. } => {
            Some(name.to_ascii_lowercase())
        }
        QuerySource::Aliased { alias, .. } => Some(crate::sql::binder::alias_row_qualifier(alias)),
        _ => None,
    }
}

/// Renames every unqualified entry to `qualifier.column`, so an unqualified
/// name inside the subquery resolves to the subquery's own columns first
/// (PostgreSQL scoping) and reaches the outer row only through its suffix.
fn qualified_outer_row(
    context: &ExistsResolutionContext<'_>,
    source: &QuerySource,
    row: &BatchRow,
    qualifier: Option<&str>,
) -> Result<BatchRow, QueryError> {
    let Some(qualifier) = qualifier else {
        if matches!(source, QuerySource::Join { .. }) {
            let env = super::source::SourceExecutionEnv {
                cassie: context.cassie,
                session: context.session,
                user_functions: context.user_functions,
                params: context.params,
                controls: context.controls,
            };
            return super::exists_projection::outer_row(&env, source, row);
        }
        return Ok(row.clone());
    };
    Ok(BatchRow::new(
        row.entries()
            .iter()
            .map(|(name, value)| {
                (
                    super::outer_names::outer_field_name(
                        &context.cassie.catalog,
                        source,
                        Some(qualifier),
                        name,
                    ),
                    value.clone(),
                )
            })
            .collect(),
    ))
}

/// Returns the qualified outer row with unqualified aliases for the columns
/// the subquery's own relations do not define, so an unqualified name
/// resolves to the innermost scope first, as in PostgreSQL.
pub(super) fn scoped_outer_row(
    context: &ExistsResolutionContext<'_>,
    subquery: &LogicalPlan,
    row: &BatchRow,
) -> Result<BatchRow, QueryError> {
    use crate::executor::retained_memory::{add, lookup_bytes, mul, value_clone_bytes};
    use std::mem::size_of;
    super::check_timeout(context.controls)?;
    let mut inner_columns = crate::runtime::accounted::AccountedVec::try_new(context.controls)?;
    collect_source_columns(context, &subquery.source, &mut inner_columns)?;
    let mut names = 0_usize;
    let mut bytes = mul(
        row.entries().len(),
        size_of::<(String, crate::types::Value)>() + size_of::<(String, usize)>(),
    )?;
    for (name, value) in row.entries() {
        super::check_timeout(context.controls)?;
        // Canonical field spelling may quote/escape the stored final component.
        let alias = add(mul(name.len(), 2)?, 2)?;
        names = add(names, add(name.len(), alias)?)?;
        bytes = add(
            bytes,
            add(add(name.len(), alias)?, value_clone_bytes(value)?)?,
        )?;
    }
    for (name, _) in row.aliases() {
        super::check_timeout(context.controls)?;
        names = add(names, name.len())?;
        bytes = add(bytes, add(size_of::<(String, usize)>(), name.len())?)?;
    }
    let alias_count = add(row.entries().len(), row.aliases().len())?;
    bytes = add(
        bytes,
        lookup_bytes(add(row.entries().len(), alias_count)?, names)?,
    )?;
    bytes = add(
        bytes,
        size_of::<crate::runtime::QueryMemoryReservation>() + 2 * size_of::<usize>(),
    )?;
    let memory = std::sync::Arc::new(context.controls.reserve_query_memory(bytes)?);
    let mut aliases = Vec::new();
    aliases
        .try_reserve_exact(alias_count)
        .map_err(|error| crate::app::CassieError::ResourceLimit(error.to_string()))?;
    for (index, (name, _)) in row.entries().iter().enumerate() {
        super::check_timeout(context.controls)?;
        let column = crate::sql::ColumnIdentifierPath::stored_row_field_key(name);
        if !inner_columns.as_slice().contains(&column) {
            aliases.push((column, index));
        }
    }
    for (name, index) in row.aliases() {
        super::check_timeout(context.controls)?;
        let qualified =
            crate::sql::ColumnIdentifierPath::parse(name).is_ok_and(|path| path.is_qualified());
        let column = crate::sql::ColumnIdentifierPath::stored_row_field_key(name);
        if qualified || !inner_columns.as_slice().contains(&column) {
            aliases.push((name.clone(), *index));
        }
    }
    let mut retained = BatchRow::with_aliases(row.entries().to_vec(), aliases)
        .with_optional_data_types(row.shared_data_types())
        .with_query_memory(row.query_memory())
        .with_operator_memory(row.operator_memory());
    retained.attach_operator_memory(context.controls, memory)?;
    Ok(retained)
}

fn collect_source_columns(
    context: &ExistsResolutionContext<'_>,
    source: &QuerySource,
    columns: &mut crate::runtime::accounted::AccountedVec<String>,
) -> Result<(), QueryError> {
    fn append<'a>(
        context: &ExistsResolutionContext<'_>,
        names: impl Iterator<Item = &'a str>,
        columns: &mut crate::runtime::accounted::AccountedVec<String>,
    ) -> Result<(), QueryError> {
        for name in names {
            super::check_timeout(context.controls)?;
            let bytes = name
                .len()
                .checked_mul(2)
                .and_then(|bytes| bytes.checked_add(2))
                .ok_or_else(|| {
                    crate::app::CassieError::ResourceLimit("EXISTS field spelling overflow".into())
                })?;
            columns.try_push_with_result(bytes, || {
                Ok::<_, crate::app::CassieError>(
                    crate::sql::ColumnIdentifierPath::stored_field_key(name),
                )
            })?;
        }
        Ok(())
    }
    super::check_timeout(context.controls)?;
    match source {
        QuerySource::Collection(name) => {
            let (schema, _memory) = context
                .cassie
                .catalog
                .clone_schema_with_controls(name, context.controls)?;
            if let Some(schema) = schema {
                append(
                    context,
                    schema.fields.iter().map(|field| field.name.as_str()),
                    columns,
                )?;
            }
        }
        QuerySource::Cte(name) => {
            if let Some(relation) = context.cte_context.get(name) {
                append(
                    context,
                    relation.fields.iter().map(|field| field.name.as_str()),
                    columns,
                )?;
            }
        }
        QuerySource::Aliased { source, .. } => collect_source_columns(context, source, columns)?,
        QuerySource::Join { left, right, .. } => {
            collect_source_columns(context, left, columns)?;
            collect_source_columns(context, right, columns)?;
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
#[path = "exists_correlated_retention_tests.rs"]
mod retention_tests;

#[cfg(test)]
#[path = "exists_correlated_joined_tests.rs"]
mod joined_tests;
