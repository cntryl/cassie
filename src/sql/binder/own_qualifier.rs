//! Removes a single-relation query's own qualifier from its column references.
//!
//! `SELECT t.name FROM t WHERE t.k = 1` names the same columns as the bare
//! spelling. Rows read from a lone relation carry bare column names, so every
//! read path (projected, indexed, top-k, aggregate and full scan) resolves the
//! bare form. Joins keep their qualifiers because their rows carry qualified
//! aliases, and lateral subqueries keep theirs because their rows are combined
//! with the outer row, where a bare name could match an outer column instead.

use crate::sql::ast::{
    DeleteStatement, Expr, OrderExpr, QuerySource, SelectItem, SelectStatement, UpdateStatement,
};

/// Rewrites `select`'s own-qualified column references to bare names when its
/// source is a single collection or CTE.
pub(super) fn strip_select_own_qualifiers(select: &mut SelectStatement) {
    let (QuerySource::Collection(name) | QuerySource::Cte(name)) = &select.source else {
        return;
    };
    let qualifiers = crate::catalog::qualifier_variants(name);
    for item in &mut select.projection {
        strip_select_item(item, &qualifiers);
    }
    strip_optional(&mut select.filter, &qualifiers);
    strip_all(&mut select.group_by, &qualifiers);
    strip_optional(&mut select.having, &qualifiers);
    strip_order(&mut select.order, &qualifiers);
    strip_all(&mut select.distinct_on, &qualifiers);
}

/// Rewrites an `UPDATE`'s own-qualified references in its `WHERE` and
/// `RETURNING` clauses and assignment values.
pub(super) fn strip_update_own_qualifiers(statement: &mut UpdateStatement) {
    let qualifiers = crate::catalog::qualifier_variants(&statement.table);
    for (_, value) in &mut statement.assignments {
        *value = strip_expr(value, &qualifiers);
    }
    strip_optional(&mut statement.filter, &qualifiers);
    for item in &mut statement.returning {
        strip_select_item(item, &qualifiers);
    }
}

/// Rewrites a `DELETE`'s own-qualified references in its `WHERE` and
/// `RETURNING` clauses.
pub(super) fn strip_delete_own_qualifiers(statement: &mut DeleteStatement) {
    let qualifiers = crate::catalog::qualifier_variants(&statement.table);
    strip_optional(&mut statement.filter, &qualifiers);
    for item in &mut statement.returning {
        strip_select_item(item, &qualifiers);
    }
}

fn strip_select_item(item: &mut SelectItem, qualifiers: &[String]) {
    match item {
        SelectItem::Column { name, .. } => strip_name(name, qualifiers),
        SelectItem::Function { function, .. } => strip_all(&mut function.args, qualifiers),
        SelectItem::Expr { expr, .. } => *expr = strip_expr(expr, qualifiers),
        SelectItem::WindowFunction { function, .. } => {
            strip_all(&mut function.args, qualifiers);
            strip_all(&mut function.partition_by, qualifiers);
            strip_order(&mut function.order_by, qualifiers);
        }
        SelectItem::Wildcard => {}
    }
}

fn strip_optional(expr: &mut Option<Expr>, qualifiers: &[String]) {
    if let Some(expr) = expr {
        *expr = strip_expr(expr, qualifiers);
    }
}

fn strip_all(exprs: &mut [Expr], qualifiers: &[String]) {
    for expr in exprs {
        *expr = strip_expr(expr, qualifiers);
    }
}

fn strip_order(order: &mut [OrderExpr], qualifiers: &[String]) {
    for item in order {
        item.expr = strip_expr(&item.expr, qualifiers);
    }
}

fn strip_expr(expr: &Expr, qualifiers: &[String]) -> Expr {
    match expr {
        Expr::Column(name) => {
            let mut name = name.clone();
            strip_name(&mut name, qualifiers);
            Expr::Column(name)
        }
        _ => expr.map_children(|child| strip_expr(child, qualifiers)),
    }
}

fn strip_name(name: &mut String, qualifiers: &[String]) {
    let Some((qualifier, column)) = name.rsplit_once('.') else {
        return;
    };
    if column.is_empty() || !qualifiers.contains(&qualifier.to_ascii_lowercase()) {
        return;
    }
    *name = column.to_string();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn qualifiers() -> Vec<String> {
        crate::catalog::qualifier_variants("postgres.public.tq")
    }

    #[test]
    fn should_strip_every_own_qualifier_variant() {
        // Arrange
        let spellings = ["tq.k", "TQ.k", "public.tq.k", "postgres.public.tq.k"];

        // Act
        let stripped = spellings
            .iter()
            .map(|spelling| {
                let mut name = (*spelling).to_string();
                strip_name(&mut name, &qualifiers());
                name
            })
            .collect::<Vec<_>>();

        // Assert
        assert_eq!(stripped, vec!["k", "k", "k", "k"]);
    }

    #[test]
    fn should_keep_references_qualified_by_another_relation() {
        // Arrange
        let mut name = "other.k".to_string();

        // Act
        strip_name(&mut name, &qualifiers());

        // Assert
        assert_eq!(name, "other.k");
    }
}
