//! Spells search and vector function field arguments the way the table
//! declares them.
//!
//! `search_score(BODY, 'q')` and `vector_distance(EMBEDDING, ...)` name their
//! field as a column reference. The scored read paths use that name as an
//! exact payload and index key, so an other-case spelling of an unquoted
//! (case-insensitive) identifier silently scored zero or matched nothing.

use crate::catalog::Catalog;
use crate::sql::ast::{Expr, FunctionCall, QuerySource, SelectItem, SelectStatement};

const FIELD_FUNCTIONS: [&str; 5] = [
    "search",
    "search_score",
    "vector_distance",
    "vector_score",
    "hybrid_score",
];

/// Rewrites the column arguments of search and vector functions in `select`
/// to the declared field spelling when its source is a single collection.
pub(super) fn canonicalize_search_field_arguments(select: &mut SelectStatement, catalog: &Catalog) {
    let QuerySource::Collection(name) = &select.source else {
        return;
    };
    let Some(schema) = catalog.get_schema(name) else {
        return;
    };
    let fields = schema
        .fields
        .iter()
        .map(|field| field.name.clone())
        .collect::<Vec<_>>();
    for item in &mut select.projection {
        match item {
            SelectItem::Function { function, .. } => canonicalize_function(function, &fields),
            SelectItem::Expr { expr, .. } => *expr = canonicalize_expr(expr, &fields),
            SelectItem::Column { .. }
            | SelectItem::WindowFunction { .. }
            | SelectItem::Wildcard => {}
        }
    }
    if let Some(filter) = &mut select.filter {
        *filter = canonicalize_expr(filter, &fields);
    }
    for order in &mut select.order {
        order.expr = canonicalize_expr(&order.expr, &fields);
    }
}

fn canonicalize_expr(expr: &Expr, fields: &[String]) -> Expr {
    if let Expr::Function(function) = expr {
        let mut function = function.clone();
        canonicalize_function(&mut function, fields);
        return Expr::Function(function);
    }
    expr.map_children(|child| canonicalize_expr(child, fields))
}

fn canonicalize_function(function: &mut FunctionCall, fields: &[String]) {
    let is_field_function = FIELD_FUNCTIONS
        .iter()
        .any(|name| function.name.eq_ignore_ascii_case(name));
    for arg in &mut function.args {
        match arg {
            Expr::Column(column) if is_field_function => {
                if let Some(declared) = declared_field(fields, column) {
                    *column = declared.to_string();
                }
            }
            _ => *arg = canonicalize_expr(arg, fields),
        }
    }
}

fn declared_field<'a>(fields: &'a [String], column: &str) -> Option<&'a str> {
    if fields.iter().any(|field| field == column) {
        return None;
    }
    fields
        .iter()
        .find(|field| field.eq_ignore_ascii_case(column))
        .map(String::as_str)
}
