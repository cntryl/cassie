//! A qualified reference binds to the innermost FROM item that defines its qualifier, so a
//! subquery alias that reuses an enclosing alias hides the enclosing table's columns.
use super::{CassieError, Catalog, CteScope, Expr, HashSet, SelectItem};
use crate::sql::ast::{ParsedStatement, QuerySource, QueryStatement};

/// Rejects a plain qualified column of `statement` whose qualifier names one of its own FROM
/// items that has no such column. Other qualifiers stay enclosing-scope references.
pub(super) fn validate(
    statement: &ParsedStatement,
    catalog: &Catalog,
    ctes: &CteScope<'_>,
) -> Result<(), CassieError> {
    let QueryStatement::Select(select) = &statement.statement else {
        return Ok(());
    };
    // Only base tables without column aliases list their columns completely here; table
    // functions, graph tables and other sources keep the diagnostics of the binding that follows.
    if !base_tables_only(&select.source) {
        return Ok(());
    }
    let Ok(fields) = super::select::source_fields(catalog, &select.source, ctes) else {
        return Ok(());
    };
    if fields.contains("*") {
        return Ok(());
    }
    let scope = Scope {
        qualifiers: fields
            .iter()
            .filter(|name| {
                !name.starts_with("__cassie_ambiguous__.")
                    && !name.starts_with("__cassie_identity__.")
            })
            .filter_map(|name| name.rsplit_once('.').map(|(qualifier, _)| qualifier))
            .collect(),
        fields: &fields,
        catalog,
        ctes,
    };
    for item in &select.projection {
        match item {
            SelectItem::Column { name, .. } => scope.column(name)?,
            SelectItem::Expr { expr, .. } => scope.expression(expr)?,
            SelectItem::Function { function, .. } => {
                function
                    .expressions()
                    .try_for_each(|expr| scope.expression(expr))?;
            }
            SelectItem::WindowFunction { .. } | SelectItem::Wildcard => {}
        }
    }
    select
        .filter
        .iter()
        .chain(&select.having)
        .chain(&select.group_by)
        .chain(select.order.iter().map(|order| &order.expr))
        .try_for_each(|expr| scope.expression(expr))
}

fn base_tables_only(source: &QuerySource) -> bool {
    match source {
        QuerySource::Collection(_) => true,
        QuerySource::Aliased {
            source,
            column_aliases,
            ..
        } => column_aliases.is_empty() && base_tables_only(source),
        QuerySource::TableSample { source, .. } => base_tables_only(source),
        QuerySource::Join { left, right, .. } => base_tables_only(left) && base_tables_only(right),
        _ => false,
    }
}

struct Scope<'a> {
    qualifiers: HashSet<&'a str>,
    fields: &'a HashSet<String>,
    catalog: &'a Catalog,
    ctes: &'a CteScope<'a>,
}

impl Scope<'_> {
    fn expression(&self, expr: &Expr) -> Result<(), CassieError> {
        match expr {
            Expr::Column(name) => return self.column(name),
            Expr::Exists(statement) => return validate(statement, self.catalog, self.ctes),
            Expr::Subquery(subquery) => validate(&subquery.query, self.catalog, self.ctes)?,
            _ => {}
        }
        expr.try_visit_children(|child| self.expression(child))
    }

    fn column(&self, name: &str) -> Result<(), CassieError> {
        let plain = name.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'.')
        });
        let Some((qualifier, field)) = name.rsplit_once('.').filter(|_| plain) else {
            return Ok(());
        };
        // An alias qualifies its fields by a carrier derived from the alias name.
        let carrier = super::alias_row_qualifier(qualifier);
        let defined_here =
            self.qualifiers.contains(qualifier) || self.qualifiers.contains(carrier.as_str());
        if !defined_here
            || self.fields.contains(name)
            || self.fields.contains(&format!("{carrier}.{field}"))
        {
            return Ok(());
        }
        Err(CassieError::UndefinedColumn(name.to_string()))
    }
}
