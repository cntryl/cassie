//! PostgreSQL wire identities for projections of generated SEARCH/CYCLE fields.

use super::{
    ColumnMeta, CommonTableExpression, DataType, Expr, HashMap, QuerySource, Schema, SelectItem,
};
use crate::sql::ColumnIdentifierPath;

/// Bound projections reference physical source fields. The displayed label
/// can be shared by unrelated expressions or by both halves of a join.
pub(crate) fn apply_recursive_cte_record_oids_for_query(
    source: &QuerySource,
    ctes: &[CommonTableExpression],
    projection: &[SelectItem],
    columns: &mut [ColumnMeta],
) {
    if !ctes
        .iter()
        .any(|cte| cte.search.is_some() || cte.cycle.is_some())
    {
        return;
    }
    let definitions: Vec<_> = ctes.iter().collect();
    // The binder expands wildcards over generated fields into their source
    // columns. Other wildcards can expand to a different number of slots.
    if projection.len() != columns.len() {
        return;
    }
    for (item, column) in projection.iter().zip(columns) {
        let Some((reference, _)) = direct_reference(item) else {
            continue;
        };
        if let Some(oid) = reference_oid(source, &definitions, reference) {
            column.type_oid = oid;
            column.is_record |= oid == 2249;
        }
    }
}

fn direct_reference(item: &SelectItem) -> Option<(&str, Option<&str>)> {
    match item {
        SelectItem::Column { name, alias }
        | SelectItem::Expr {
            expr: Expr::Column(name),
            alias,
        } => Some((name, alias.as_deref())),
        _ => None,
    }
}

fn reference_oid(
    source: &QuerySource,
    ctes: &[&CommonTableExpression],
    reference: &str,
) -> Option<i64> {
    let path = ColumnIdentifierPath::parse(reference).ok()?;
    let qualifier = path.namespace_qualifier();
    if path.is_qualified() && qualifier.is_none() {
        return None;
    }
    source_oid(source, ctes, qualifier.as_deref(), &path.declared_name())
}

fn source_oid(
    source: &QuerySource,
    ctes: &[&CommonTableExpression],
    qualifier: Option<&str>,
    field: &str,
) -> Option<i64> {
    match source {
        QuerySource::Cte(name) if qualifier.is_none_or(|owner| owner == name) => {
            let cte = ctes.iter().rev().find(|cte| &cte.name == name)?;
            generated_oid(cte, field)
        }
        QuerySource::Aliased { source, alias, .. }
            if qualifier
                .is_none_or(|owner| owner == crate::sql::binder::alias_row_qualifier(alias)) =>
        {
            source_oid(source, ctes, None, field)
        }
        QuerySource::Join { left, right, .. } => source_oid(left, ctes, qualifier, field)
            .or_else(|| source_oid(right, ctes, qualifier, field)),
        QuerySource::TableSample { source, .. } => source_oid(source, ctes, qualifier, field),
        QuerySource::Subquery { alias, select, .. }
            if qualifier.is_none_or(|owner| owner == alias) =>
        {
            let definitions: Vec<_> = ctes.iter().copied().chain(&select.ctes).collect();
            select.projection.iter().find_map(|item| {
                if matches!(item, SelectItem::Wildcard) {
                    return source_oid(&select.source, &definitions, None, field);
                }
                let (reference, label) = direct_reference(item)?;
                let path = ColumnIdentifierPath::parse(reference).ok()?;
                if label.unwrap_or(path.final_name()) != field {
                    return None;
                }
                reference_oid(&select.source, &definitions, reference)
            })
        }
        _ => None,
    }
}

fn generated_oid(cte: &CommonTableExpression, field: &str) -> Option<i64> {
    if let Some(search) = &cte.search {
        if search.sequence_column == field {
            return Some(match search.order {
                crate::sql::ast::CteSearchOrder::DepthFirst => 2287,
                crate::sql::ast::CteSearchOrder::BreadthFirst => 2249,
            });
        }
    }
    let cycle = cte.cycle.as_ref()?;
    if cycle.path_column == field {
        return Some(2287);
    }
    if cycle.mark_column != field {
        return None;
    }
    let schema = Schema { fields: Vec::new() };
    let functions = HashMap::new();
    let mark = crate::sql::binder::infer_expr_type(&cycle.mark_value, &schema, &functions, &[])?;
    let default =
        crate::sql::binder::infer_expr_type(&cycle.default_value, &schema, &functions, &[])?;
    let common = match (mark, default) {
        (DataType::Null, DataType::Null) => DataType::Text,
        (DataType::Null, other) | (other, DataType::Null) => other,
        (mark, default) => crate::sql::binder::common_case_type(mark, default)?,
    };
    Some(common.type_oid())
}
