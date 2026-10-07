//! CTE column aliases and their ordered row shape.

use super::{CassieError, Catalog, CteQuery, HashMap, QuerySource};

pub(super) fn binding(
    cte: &crate::sql::ast::CommonTableExpression,
    previous: &[crate::sql::ast::CommonTableExpression],
    scope: &super::super::CteScope,
    catalog: &Catalog,
    context: &super::super::BindingContext,
) -> Result<super::super::CteBinding, CassieError> {
    let functions = crate::catalog::function_resolution::functions_for_scope(
        &catalog.list_functions(),
        &context.database,
        &context.search_path,
        context.scopes_database_objects(),
    );
    let row_scope = scope
        .iter()
        .map(|(name, binding)| (name.clone(), binding.row_fields.clone()))
        .collect();
    let row_fields = super::super::wildcard::cte_row_fields(cte, &row_scope, catalog, &functions)?;
    let mut ctes = previous.to_vec();
    ctes.push(cte.clone());
    let schema = super::super::source_schema::derived_source_schema(
        &QuerySource::Cte(cte.name.clone()),
        &ctes,
        catalog,
        &functions,
    )
    .ok_or_else(|| CassieError::Planner("cannot infer CTE output columns".into()))?;
    let visible = schema
        .fields
        .into_iter()
        .map(|field| field.name)
        .collect::<Vec<_>>();
    if visible.len() != row_fields.len() {
        return Err(CassieError::Planner(
            "CTE output columns do not match row shape".into(),
        ));
    }
    Ok(super::super::CteBinding {
        visible,
        row_fields,
    })
}

/// Returns a CTE's visible column names and the alias list stored on the
/// bound CTE. Without a declared list both are the body's output names (`*`
/// for a wildcard body). A declared list may be shorter than the body's
/// output, as in PostgreSQL: it renames the leading columns and the rest keep
/// their names. A longer list, or any mismatch for a recursive CTE, is an
/// error.
pub(super) fn cte_column_aliases(
    cte_name: &str,
    declared: &[String],
    query: &CteQuery,
    bound_ctes: &[crate::sql::ast::CommonTableExpression],
    catalog: &Catalog,
) -> Result<(Vec<String>, Vec<String>), CassieError> {
    let visible = super::cte_output_fields(query)?;
    if declared.is_empty() {
        return Ok((visible.clone(), visible));
    }
    let recursive = matches!(query, CteQuery::Recursive { .. });
    let visible = if visible.len() == 1 && visible[0] == "*" {
        wildcard_cte_columns(cte_name, query, bound_ctes, catalog).unwrap_or(visible)
    } else {
        visible
    };
    let known_width = !(visible.len() == 1 && visible[0] == "*");
    if known_width
        && (declared.len() > visible.len() || (recursive && declared.len() != visible.len()))
    {
        return Err(CassieError::Planner(format!(
            "CTE '{cte_name}' alias count does not match output columns"
        )));
    }
    let rest: Vec<String> = if known_width && !recursive {
        visible.into_iter().skip(declared.len()).collect()
    } else {
        Vec::new()
    };
    let stored: Vec<String> = declared.iter().cloned().chain(rest).collect();
    Ok((stored.clone(), stored))
}

/// The output column names of a wildcard CTE body, in row order.
fn wildcard_cte_columns(
    cte_name: &str,
    query: &CteQuery,
    bound_ctes: &[crate::sql::ast::CommonTableExpression],
    catalog: &Catalog,
) -> Option<Vec<String>> {
    let mut in_scope = bound_ctes.to_vec();
    in_scope.push(crate::sql::ast::CommonTableExpression {
        name: cte_name.to_string(),
        aliases: Vec::new(),
        query: query.clone(),
    });
    let schema = super::super::source_schema::derived_source_schema(
        &QuerySource::Cte(cte_name.to_string()),
        &in_scope,
        catalog,
        &HashMap::new(),
    )?;
    Some(schema.fields.into_iter().map(|field| field.name).collect())
}
