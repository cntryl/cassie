use crate::sql::ast::{
    CommonTableExpression, CteQuery, Expr, QuerySource, QueryStatement, SelectItem, SelectStatement,
};
use crate::sql::ColumnIdentifierPath;

use super::FunctionId;

/// Identifies a direct percentile-array output column exposed by a CTE or
/// subquery. General SQL array columns keep their existing private identity.
pub(crate) fn percentile_function_for_column(
    source: &QuerySource,
    ctes: &[CommonTableExpression],
    column_name: &str,
) -> Option<FunctionId> {
    match source {
        QuerySource::Cte(name) => {
            let cte = ctes
                .iter()
                .find(|cte| cte.name.eq_ignore_ascii_case(name))?;
            let select = cte_select(cte)?;
            select
                .projection
                .iter()
                .enumerate()
                .find(|(index, item)| {
                    let output_name = cte
                        .aliases
                        .get(*index)
                        .cloned()
                        .or_else(|| projection_output_name(item));
                    output_name
                        .is_some_and(|output_name| same_column_name(&output_name, column_name))
                })
                .and_then(|(_, item)| direct_percentile_function(item))
        }
        QuerySource::Subquery { select, .. } => {
            projection_function_for_column(&select.projection, &[], column_name)
        }
        QuerySource::Aliased {
            source,
            column_aliases,
            ..
        } if !column_aliases.is_empty() => {
            let index = column_aliases
                .iter()
                .position(|alias| same_column_name(alias, column_name))?;
            direct_percentile_function_for_source_index(source, ctes, index)
        }
        QuerySource::Aliased { source, .. } => {
            percentile_function_for_column(source, ctes, column_name)
        }
        _ => None,
    }
}

fn direct_percentile_function_for_source_index(
    source: &QuerySource,
    ctes: &[CommonTableExpression],
    index: usize,
) -> Option<FunctionId> {
    match source {
        QuerySource::Cte(name) => {
            let cte = ctes
                .iter()
                .find(|cte| cte.name.eq_ignore_ascii_case(name))?;
            direct_percentile_function(cte_select(cte)?.projection.get(index)?)
        }
        QuerySource::Subquery { select, .. } => {
            direct_percentile_function(select.projection.get(index)?)
        }
        QuerySource::Aliased { source, .. } => {
            direct_percentile_function_for_source_index(source, ctes, index)
        }
        _ => None,
    }
}

fn projection_function_for_column(
    projection: &[SelectItem],
    aliases: &[String],
    column_name: &str,
) -> Option<FunctionId> {
    projection
        .iter()
        .enumerate()
        .find(|(index, item)| {
            let output_name = aliases
                .get(*index)
                .cloned()
                .or_else(|| projection_output_name(item));
            output_name.is_some_and(|output_name| same_column_name(&output_name, column_name))
        })
        .and_then(|(_, item)| direct_percentile_function(item))
}

fn cte_select(cte: &CommonTableExpression) -> Option<&SelectStatement> {
    let statement = match &cte.query {
        CteQuery::Simple(statement) => statement,
        CteQuery::Recursive { base, .. } => base,
    };
    let QueryStatement::Select(select) = &statement.statement else {
        return None;
    };
    Some(select)
}

fn projection_output_name(item: &SelectItem) -> Option<String> {
    match item {
        SelectItem::Wildcard => None,
        SelectItem::Column { name, alias } => alias.clone().or_else(|| {
            ColumnIdentifierPath::parse(name)
                .ok()
                .map(|column| column.implicit_output_label())
        }),
        SelectItem::Function { function, alias } => {
            Some(alias.clone().unwrap_or_else(|| function.name.clone()))
        }
        SelectItem::Expr { alias, .. } => Some(alias.clone().unwrap_or_else(|| "expr".into())),
        SelectItem::WindowFunction { function, alias } => {
            Some(alias.clone().unwrap_or_else(|| function.name.clone()))
        }
    }
}

fn direct_percentile_function(item: &SelectItem) -> Option<FunctionId> {
    let function = match item {
        SelectItem::Function { function, .. } => function,
        SelectItem::Expr {
            expr: Expr::Function(function),
            ..
        } => function,
        _ => return None,
    };
    let metadata = super::function(&function.name)?;
    matches!(
        metadata.id,
        FunctionId::PercentileCont | FunctionId::PercentileDisc
    )
    .then_some(metadata.id)
}

fn same_column_name(left: &str, right: &str) -> bool {
    ColumnIdentifierPath::from_field_name(left).field_lookup_key()
        == ColumnIdentifierPath::from_field_name(right).field_lookup_key()
}
