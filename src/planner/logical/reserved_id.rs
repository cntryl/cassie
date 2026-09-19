//! Every row Cassie builds carries an internal document identity. Nearly
//! every read-path optimization (point lookups, keyset pagination, covering
//! indexes, projected/column-batch scan field selection) assumes any
//! reference to a column literally named `id` (or `_id`) means that reserved
//! identity, never a real stored field — see the many `is_row_id_column`
//! call sites in `crate::planner::physical` and
//! `crate::executor::execution::projected_read`.
//!
//! When a table declares its own `id` field, that assumption is simply
//! wrong, and was previously silently wrong: `WHERE id = 42`, `ORDER BY id`,
//! and `SELECT id` all resolved to the internal identity instead of the
//! user's value, because the optimizations can't tell the two cases apart
//! from a bare column name alone.
//!
//! Rather than threading schema awareness through every one of those call
//! sites, this rewrites the query at the logical-plan level, once, right
//! after it's built: for a plain single-table query whose target schema
//! does *not* declare its own `id` field, every bare `id` reference is
//! renamed to `_id` (the name that already means "the reserved identity"
//! everywhere else, e.g. `INSERT ... RETURNING *`). Every existing
//! optimization then keeps working unchanged, because it's matching `_id`
//! instead of `id`. When the schema *does* declare its own `id` field, no
//! rewrite happens, so `id` simply stops matching any of those
//! optimizations' `is_row_id_column` checks and is treated as an ordinary
//! column everywhere — which is exactly correct.
//!
//! Scope: every source shape that can put the internal identity in a row
//! is rewritten, because the row shape itself is global — `scan` stores the
//! identity only under `_id`, so any `id` reference left unrewritten
//! resolves to nothing at all. A bare `id` is rewritten whenever *nothing*
//! in scope declares its own `id`: a single collection, both sides of a
//! join, and the body of a subquery or CTE reference (recursively). A
//! subquery or CTE that projects an explicit `id` output column already
//! carries a real `id` entry, so references to it are left alone.

use super::LogicalPlan;
use crate::catalog::Catalog;
use crate::sql::ast::{
    CommonTableExpression, CteQuery, Expr, OrderExpr, QuerySource, QueryStatement, SelectItem,
    SelectStatement,
};

const RESERVED_ID: &str = "id";
const INTERNAL_IDENTITY: &str = "_id";

/// Whether `collection`'s schema declares its own `id` field, i.e. whether
/// a bare `id` reference against it means the user's field rather than
/// Cassie's reserved internal identity. Shared with callers that evaluate a
/// filter expression directly against constructed rows instead of going
/// through a `LogicalPlan` (see `rewrite_expr_for_schema`).
#[must_use]
pub fn collection_declares_id(catalog: &Catalog, collection: &str) -> bool {
    catalog.get_schema(collection).is_some_and(|schema| {
        schema
            .fields
            .iter()
            .any(|field| is_reserved_id(&field.name))
    })
}

/// Rewrites a single expression in place exactly as
/// `rewrite_reserved_id_references` would, for callers (DML `WHERE`
/// evaluation) that evaluate an expression directly against constructed
/// rows rather than going through a `LogicalPlan`. `schema_has_id` should
/// come from `collection_declares_id` for the expression's target table.
pub fn rewrite_expr_for_schema(expr: &mut Expr, schema_has_id: bool) {
    if schema_has_id {
        return;
    }
    rewrite_expr(expr);
}

pub fn rewrite_reserved_id_references(plan: &mut LogicalPlan, catalog: &Catalog) {
    if !source_resolves_id_to_identity(&plan.source, &plan.ctes, catalog) {
        return;
    }

    for item in &mut plan.projection {
        rewrite_select_item(item);
    }
    if let Some(filter) = &mut plan.filter {
        rewrite_expr(filter);
    }
    for expr in &mut plan.group_by {
        rewrite_expr(expr);
    }
    if let Some(having) = &mut plan.having {
        rewrite_expr(having);
    }
    for order in &mut plan.order {
        rewrite_order_expr(order);
    }
    for expr in &mut plan.distinct_on {
        rewrite_expr(expr);
    }
}

/// Whether a bare `id` reference against `source` means Cassie's reserved
/// internal identity rather than a real column that something in scope
/// declares or projects. Only then is rewriting `id` to `_id` correct.
fn source_resolves_id_to_identity(
    source: &QuerySource,
    ctes: &[CommonTableExpression],
    catalog: &Catalog,
) -> bool {
    match source {
        QuerySource::Collection(collection) => !collection_declares_id(catalog, collection),
        // A join row carries each side's entries, so `id` means the identity
        // only when neither side declares one of its own.
        QuerySource::Join { left, right, .. } => {
            source_resolves_id_to_identity(left, ctes, catalog)
                && source_resolves_id_to_identity(right, ctes, catalog)
        }
        QuerySource::Subquery { select, .. } => {
            inner_resolves_id_to_identity(select, ctes, catalog)
        }
        QuerySource::Cte(name) => ctes
            .iter()
            .find(|cte| cte.name.eq_ignore_ascii_case(name))
            .and_then(cte_body)
            .is_some_and(|select| inner_resolves_id_to_identity(select, ctes, catalog)),
        // A table function's rows and the single synthetic row have no
        // document identity to resolve to, so `id` is left as written.
        QuerySource::TableFunction { .. } | QuerySource::SingleRow => false,
    }
}

/// A subquery or CTE body that projects an explicit `id` output column
/// hands its consumer a real `id` entry (a rewritten `_id AS id`, or a
/// user's own aliased value), so an outer reference must not be rewritten.
/// Otherwise the body just passes its source's rows through and the same
/// question applies one level down.
fn inner_resolves_id_to_identity(
    select: &SelectStatement,
    outer_ctes: &[CommonTableExpression],
    catalog: &Catalog,
) -> bool {
    if projects_id_output(select) {
        return false;
    }
    let ctes = if select.ctes.is_empty() {
        outer_ctes
    } else {
        &select.ctes
    };
    source_resolves_id_to_identity(&select.source, ctes, catalog)
}

fn cte_body(cte: &CommonTableExpression) -> Option<&SelectStatement> {
    let statement = match &cte.query {
        CteQuery::Simple(statement) => statement,
        // A recursive CTE's output shape is its base branch's shape.
        CteQuery::Recursive { base, .. } => base,
    };
    match &statement.statement {
        QueryStatement::Select(select) => Some(select),
        _ => None,
    }
}

fn projects_id_output(select: &SelectStatement) -> bool {
    select.projection.iter().any(|item| match item {
        SelectItem::Column { name, alias } => alias
            .as_deref()
            .map_or_else(|| is_reserved_id(name), is_reserved_id),
        SelectItem::Function { alias, .. }
        | SelectItem::Expr { alias, .. }
        | SelectItem::WindowFunction { alias, .. } => alias.as_deref().is_some_and(is_reserved_id),
        SelectItem::Wildcard => false,
    })
}

fn rewrite_select_item(item: &mut SelectItem) {
    match item {
        SelectItem::Column { name, alias } => {
            if is_reserved_id(name) {
                if alias.is_none() {
                    *alias = Some(name.clone());
                }
                *name = INTERNAL_IDENTITY.to_string();
            }
        }
        SelectItem::Function { function, .. } => {
            for arg in &mut function.args {
                rewrite_expr(arg);
            }
        }
        SelectItem::Expr { expr, .. } => rewrite_expr(expr),
        SelectItem::WindowFunction { function, .. } => {
            for arg in &mut function.args {
                rewrite_expr(arg);
            }
        }
        SelectItem::Wildcard => {}
    }
}

fn rewrite_order_expr(order: &mut OrderExpr) {
    rewrite_expr(&mut order.expr);
}

fn rewrite_expr(expr: &mut Expr) {
    match expr {
        Expr::Case {
            operand,
            branches,
            else_expr,
        } => {
            if let Some(operand) = operand {
                rewrite_expr(operand);
            }
            for (when, then) in branches {
                rewrite_expr(when);
                rewrite_expr(then);
            }
            if let Some(else_expr) = else_expr {
                rewrite_expr(else_expr);
            }
        }
        Expr::Column(name) => {
            if is_reserved_id(name) {
                *name = INTERNAL_IDENTITY.to_string();
            }
        }
        Expr::Binary { left, right, .. } => {
            rewrite_expr(left);
            rewrite_expr(right);
        }
        Expr::IsNull { expr, .. } | Expr::Not { expr } | Expr::Cast { expr, .. } => {
            rewrite_expr(expr);
        }
        Expr::InList { expr, values, .. } => {
            rewrite_expr(expr);
            for value in values {
                rewrite_expr(value);
            }
        }
        Expr::Between {
            expr, low, high, ..
        } => {
            rewrite_expr(expr);
            rewrite_expr(low);
            rewrite_expr(high);
        }
        Expr::Function(function) => {
            for arg in &mut function.args {
                rewrite_expr(arg);
            }
        }
        // A subquery has its own, potentially different, target schema; it
        // is out of scope (see module docs) and left unrewritten.
        Expr::Exists(_)
        | Expr::Param(_)
        | Expr::StringLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::IntegerLiteral(_)
        | Expr::BoolLiteral(_)
        | Expr::Null => {}
    }
}

fn is_reserved_id(name: &str) -> bool {
    name.eq_ignore_ascii_case(RESERVED_ID)
}

#[cfg(test)]
mod tests {
    use super::rewrite_reserved_id_references;
    use crate::catalog::Catalog;
    use crate::planner::LogicalPlan;
    use crate::sql::ast::{
        BinaryOp, Expr, NullsOrder, OrderExpr, QuerySource, SelectItem, SortDirection,
    };
    use crate::types::DataType;

    fn id_lookup_plan(collection: &str) -> LogicalPlan {
        LogicalPlan {
            command: None,
            source: QuerySource::Collection(collection.to_string()),
            collection: collection.to_string(),
            ctes: Vec::new(),
            distinct: false,
            distinct_on: Vec::new(),
            projection: vec![
                SelectItem::Column {
                    name: "id".to_string(),
                    alias: None,
                },
                SelectItem::Column {
                    name: "name".to_string(),
                    alias: None,
                },
            ],
            filter: Some(Expr::Binary {
                left: Box::new(Expr::Column("id".to_string())),
                op: BinaryOp::Eq,
                right: Box::new(Expr::IntegerLiteral(42)),
            }),
            group_by: Vec::new(),
            having: None,
            order: vec![OrderExpr {
                expr: Expr::Column("id".to_string()),
                direction: SortDirection::Asc,
                nulls: None::<NullsOrder>,
            }],
            limit: None,
            offset: None,
            set: None,
        }
    }

    #[test]
    fn should_rewrite_bare_id_when_schema_has_no_id_field() {
        // Arrange
        let catalog = Catalog::new();
        catalog.register_collection("t", vec![("name".to_string(), DataType::Text)]);
        let mut plan = id_lookup_plan("t");

        // Act
        rewrite_reserved_id_references(&mut plan, &catalog);

        // Assert
        let SelectItem::Column { name, alias } = &plan.projection[0] else {
            panic!("expected column projection");
        };
        assert_eq!(name, "_id");
        assert_eq!(alias.as_deref(), Some("id"));
        assert!(matches!(
            &plan.filter,
            Some(Expr::Binary { left, .. }) if matches!(left.as_ref(), Expr::Column(name) if name == "_id")
        ));
        assert!(matches!(
            &plan.order[0].expr,
            Expr::Column(name) if name == "_id"
        ));
    }

    #[test]
    fn should_not_rewrite_id_when_schema_declares_its_own_id_field() {
        // Arrange
        let catalog = Catalog::new();
        catalog.register_collection(
            "t",
            vec![
                ("id".to_string(), DataType::Int),
                ("name".to_string(), DataType::Text),
            ],
        );
        let mut plan = id_lookup_plan("t");

        // Act
        rewrite_reserved_id_references(&mut plan, &catalog);

        // Assert
        let SelectItem::Column { name, alias } = &plan.projection[0] else {
            panic!("expected column projection");
        };
        assert_eq!(name, "id");
        assert_eq!(*alias, None);
        assert!(matches!(
            &plan.filter,
            Some(Expr::Binary { left, .. }) if matches!(left.as_ref(), Expr::Column(name) if name == "id")
        ));
        assert!(matches!(
            &plan.order[0].expr,
            Expr::Column(name) if name == "id"
        ));
    }

    #[test]
    fn should_not_rewrite_ordinary_expressions_beyond_id() {
        // Arrange
        let catalog = Catalog::new();
        catalog.register_collection("t", vec![("name".to_string(), DataType::Text)]);
        let mut plan = id_lookup_plan("t");

        // Act
        rewrite_reserved_id_references(&mut plan, &catalog);

        // Assert
        let SelectItem::Column { name, .. } = &plan.projection[1] else {
            panic!("expected column projection");
        };
        assert_eq!(name, "name");
    }
}
