use super::{BinaryOp, Expr, IndexKind, IndexMeta, LogicalPlan, QuerySource};
use crate::sql::ast::SortDirection;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScalarIndexPlanPath {
    IndexSeek,
    PrefixScan,
    RangeScan,
    OrderedBoundedScan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ScalarIndexPlanShape {
    pub path: ScalarIndexPlanPath,
    /// Number of leading scalar-index key components constrained by equality.
    /// For expression indexes this counts fields first, then expression keys.
    pub equality_prefix_len: usize,
    pub range_field_index: Option<usize>,
    pub order_columns_used: usize,
    pub order_by_row_id: bool,
    pub reverse: bool,
    pub order_satisfied: bool,
}

#[derive(Debug, Clone, Default)]
struct FieldConstraintShape {
    equality: bool,
    lower: bool,
    upper: bool,
}

impl FieldConstraintShape {
    fn has_range(&self) -> bool {
        self.lower || self.upper
    }
}

pub(crate) fn scalar_index_plan_shape(
    plan: &LogicalPlan,
    index: &IndexMeta,
) -> Option<ScalarIndexPlanShape> {
    if plan.command.is_some()
        || !plan.ctes.is_empty()
        || plan.distinct
        || !plan.distinct_on.is_empty()
        || !plan.group_by.is_empty()
        || plan.having.is_some()
        || plan.set.is_some()
        || !matches!(plan.source, QuerySource::Collection(_))
        || index.kind != IndexKind::Scalar
    {
        return None;
    }

    if !index.expressions.is_empty() {
        return expression_index_plan_shape(plan, index);
    }

    let constraints = filter_constraint_shapes(plan.filter.as_ref())?;
    let fields = index.normalized_fields();
    if fields.is_empty()
        || constraints.keys().any(|field| {
            !fields
                .iter()
                .any(|candidate| candidate.eq_ignore_ascii_case(field))
        })
    {
        return None;
    }

    let equality_prefix_len = fields
        .iter()
        .take_while(|field| {
            constraints
                .get(&field.to_ascii_lowercase())
                .is_some_and(|constraint| constraint.equality)
        })
        .count();
    let range_field_index = fields
        .get(equality_prefix_len)
        .and_then(|field| constraints.get(&field.to_ascii_lowercase()))
        .and_then(|constraint| constraint.has_range().then_some(equality_prefix_len));
    let order_shape = order_shape(plan, &fields, equality_prefix_len)?;

    if equality_prefix_len == fields.len() {
        let path = if fields.len() == 1 && !order_shape.order_by_row_id {
            ScalarIndexPlanPath::IndexSeek
        } else {
            ScalarIndexPlanPath::PrefixScan
        };
        return Some(ScalarIndexPlanShape {
            path,
            equality_prefix_len,
            range_field_index,
            order_columns_used: order_shape.order_columns_used,
            order_by_row_id: order_shape.order_by_row_id,
            reverse: order_shape.reverse,
            order_satisfied: order_shape.order_satisfied,
        });
    }

    if let Some(range_field_index) = range_field_index {
        return Some(ScalarIndexPlanShape {
            path: ScalarIndexPlanPath::RangeScan,
            equality_prefix_len,
            range_field_index: Some(range_field_index),
            order_columns_used: order_shape.order_columns_used,
            order_by_row_id: order_shape.order_by_row_id,
            reverse: order_shape.reverse,
            order_satisfied: order_shape.order_satisfied,
        });
    }

    if order_shape.order_columns_used > 0 && plan.limit.is_some() {
        if !order_shape.order_satisfied && equality_prefix_len > 0 {
            return Some(ScalarIndexPlanShape {
                path: ScalarIndexPlanPath::PrefixScan,
                equality_prefix_len,
                range_field_index: None,
                order_columns_used: order_shape.order_columns_used,
                order_by_row_id: order_shape.order_by_row_id,
                reverse: order_shape.reverse,
                order_satisfied: false,
            });
        }

        if !order_shape.order_satisfied {
            return None;
        }

        return Some(ScalarIndexPlanShape {
            path: ScalarIndexPlanPath::OrderedBoundedScan,
            equality_prefix_len,
            range_field_index: None,
            order_columns_used: order_shape.order_columns_used,
            order_by_row_id: order_shape.order_by_row_id,
            reverse: order_shape.reverse,
            order_satisfied: true,
        });
    }

    None
}

pub(crate) fn scalar_index_order_proof_missing_candidate(
    plan: &LogicalPlan,
    index: &IndexMeta,
) -> bool {
    if plan.order.is_empty()
        || plan.limit.is_none()
        || index.kind != IndexKind::Scalar
        || !index.expressions.is_empty()
        || scalar_index_plan_shape(plan, index).is_some()
    {
        return false;
    }

    let Some(constraints) = filter_constraint_shapes(plan.filter.as_ref()) else {
        return false;
    };
    let fields = index.normalized_fields();
    if fields.is_empty()
        || constraints.keys().any(|field| {
            !fields
                .iter()
                .any(|candidate| candidate.eq_ignore_ascii_case(field))
        })
        || plan.order.iter().any(|order| order.nulls.is_some())
    {
        return false;
    }

    let equality_prefix_len = fields
        .iter()
        .take_while(|field| {
            constraints
                .get(&field.to_ascii_lowercase())
                .is_some_and(|constraint| constraint.equality)
        })
        .count();
    if equality_prefix_len == 0 || equality_prefix_len >= fields.len() {
        return false;
    }

    let order_terms = plan
        .order
        .iter()
        .map(|order| match &order.expr {
            Expr::Column(column) => Some((
                column.clone(),
                matches!(order.direction, SortDirection::Desc),
            )),
            _ => None,
        })
        .collect::<Option<Vec<_>>>();
    let Some(order_terms) = order_terms else {
        return false;
    };
    let effective_order = order_terms
        .into_iter()
        .filter(|(column, _)| {
            !fields[..equality_prefix_len]
                .iter()
                .any(|field| field.eq_ignore_ascii_case(column))
        })
        .collect::<Vec<_>>();
    if effective_order.len() < 2 {
        return false;
    }

    let remaining = &fields[equality_prefix_len..];
    let matched = remaining
        .iter()
        .zip(effective_order.iter())
        .take_while(|(field, (column, _))| field.eq_ignore_ascii_case(column))
        .count();
    if matched != effective_order.len() {
        return false;
    }

    let first_reverse = effective_order[0].1;
    effective_order
        .iter()
        .any(|(_, reverse)| *reverse != first_reverse)
}

fn expression_index_plan_shape(
    plan: &LogicalPlan,
    index: &IndexMeta,
) -> Option<ScalarIndexPlanShape> {
    let fields = index.normalized_fields();
    let expressions = index.normalized_expressions();
    let key_count = fields.len() + expressions.len();
    if key_count == 0 {
        return None;
    }

    if fields.is_empty() && expressions.len() == 1 {
        let order_shape = single_expression_order_shape(plan, &expressions[0])?;
        if plan.filter.is_none() && order_shape.order_columns_used > 0 && plan.limit.is_some() {
            return Some(ScalarIndexPlanShape {
                path: ScalarIndexPlanPath::OrderedBoundedScan,
                equality_prefix_len: 0,
                range_field_index: None,
                order_columns_used: order_shape.order_columns_used,
                order_by_row_id: false,
                reverse: order_shape.reverse,
                order_satisfied: order_shape.order_satisfied,
            });
        }

        if let Some(constraint) =
            single_expression_constraint_shape(plan.filter.as_ref(), &expressions[0])
        {
            if constraint.equality {
                return Some(ScalarIndexPlanShape {
                    path: ScalarIndexPlanPath::IndexSeek,
                    equality_prefix_len: 1,
                    range_field_index: None,
                    order_columns_used: order_shape.order_columns_used,
                    order_by_row_id: false,
                    reverse: order_shape.reverse,
                    order_satisfied: order_shape.order_satisfied,
                });
            }
            if constraint.has_range() {
                return Some(ScalarIndexPlanShape {
                    path: ScalarIndexPlanPath::RangeScan,
                    equality_prefix_len: 0,
                    range_field_index: Some(0),
                    order_columns_used: order_shape.order_columns_used,
                    order_by_row_id: false,
                    reverse: order_shape.reverse,
                    order_satisfied: order_shape.order_satisfied,
                });
            }
        }
    }

    if !plan.order.is_empty() {
        return None;
    }

    let equality = exact_expression_index_equalities(plan.filter.as_ref())?;
    let required_fields = fields
        .iter()
        .map(|field| field.to_ascii_lowercase())
        .collect::<BTreeSet<_>>();
    let required_expressions = expressions.into_iter().collect::<BTreeSet<_>>();
    if equality.fields != required_fields || equality.expressions != required_expressions {
        return None;
    }

    Some(ScalarIndexPlanShape {
        path: if key_count == 1 {
            ScalarIndexPlanPath::IndexSeek
        } else {
            ScalarIndexPlanPath::PrefixScan
        },
        equality_prefix_len: key_count,
        range_field_index: None,
        order_columns_used: 0,
        order_by_row_id: false,
        reverse: false,
        order_satisfied: true,
    })
}

fn single_expression_order_shape(plan: &LogicalPlan, expression: &str) -> Option<OrderShape> {
    if plan.order.is_empty() {
        return Some(OrderShape::default());
    }

    if plan.order.len() != 1 || plan.order.iter().any(|order| order.nulls.is_some()) {
        return None;
    }

    let order = &plan.order[0];
    let candidate = serde_json::to_string(&order.expr).ok()?;
    if candidate != expression {
        return None;
    }

    Some(OrderShape {
        order_columns_used: 1,
        order_by_row_id: false,
        reverse: matches!(order.direction, SortDirection::Desc),
        order_satisfied: true,
    })
}

fn single_expression_constraint_shape(
    filter: Option<&Expr>,
    expression: &str,
) -> Option<FieldConstraintShape> {
    let mut constraint = FieldConstraintShape::default();
    collect_single_expression_constraint_shape(filter?, expression, &mut constraint)?;
    Some(constraint)
}

fn collect_single_expression_constraint_shape(
    expr: &Expr,
    expression: &str,
    constraint: &mut FieldConstraintShape,
) -> Option<()> {
    match expr {
        Expr::Binary {
            left,
            op: BinaryOp::And,
            right,
        } => {
            collect_single_expression_constraint_shape(left, expression, constraint)?;
            collect_single_expression_constraint_shape(right, expression, constraint)
        }
        Expr::Binary { left, op, right } => {
            let (candidate, normalized) = expression_constraint_shape(left, op, right)?;
            if candidate != expression {
                return None;
            }
            match normalized {
                BinaryOp::Eq => constraint.equality = true,
                BinaryOp::Gt | BinaryOp::Gte => constraint.lower = true,
                BinaryOp::Lt | BinaryOp::Lte => constraint.upper = true,
                _ => return None,
            }
            Some(())
        }
        Expr::Between {
            expr,
            low,
            high,
            negated: false,
        } if super::expr_has_column(expr) && !matches!(expr.as_ref(), Expr::Column(_)) => {
            if !super::expr_is_constant(low) || !super::expr_is_constant(high) {
                return None;
            }
            let candidate = serde_json::to_string(expr.as_ref()).ok()?;
            if candidate != expression {
                return None;
            }
            constraint.lower = true;
            constraint.upper = true;
            Some(())
        }
        _ => None,
    }
}

fn expression_constraint_shape(
    left: &Expr,
    op: &BinaryOp,
    right: &Expr,
) -> Option<(String, BinaryOp)> {
    match (left, right) {
        (expr, value)
            if super::expr_has_column(expr)
                && !matches!(expr, Expr::Column(_))
                && super::expr_is_constant(value) =>
        {
            Some((serde_json::to_string(expr).ok()?, op.clone()))
        }
        (value, expr)
            if super::expr_has_column(expr)
                && !matches!(expr, Expr::Column(_))
                && super::expr_is_constant(value) =>
        {
            Some((serde_json::to_string(expr).ok()?, reverse_binary_op(op)?))
        }
        _ => None,
    }
}

#[derive(Debug, Default)]
struct ExactExpressionIndexEqualities {
    fields: BTreeSet<String>,
    expressions: BTreeSet<String>,
}

fn exact_expression_index_equalities(
    filter: Option<&Expr>,
) -> Option<ExactExpressionIndexEqualities> {
    let mut equality = ExactExpressionIndexEqualities::default();
    collect_exact_expression_index_equalities(filter?, &mut equality)?;
    Some(equality)
}

fn collect_exact_expression_index_equalities(
    expr: &Expr,
    equality: &mut ExactExpressionIndexEqualities,
) -> Option<()> {
    match expr {
        Expr::Binary {
            left,
            op: BinaryOp::And,
            right,
        } => {
            collect_exact_expression_index_equalities(left, equality)?;
            collect_exact_expression_index_equalities(right, equality)
        }
        Expr::Binary {
            left,
            op: BinaryOp::Eq,
            right,
        } => collect_exact_expression_index_equality(left, right, equality),
        _ => None,
    }
}

fn collect_exact_expression_index_equality(
    left: &Expr,
    right: &Expr,
    equality: &mut ExactExpressionIndexEqualities,
) -> Option<()> {
    match (left, right) {
        (Expr::Column(field), value) if super::expr_is_constant(value) => {
            equality.fields.insert(field.to_ascii_lowercase());
            Some(())
        }
        (value, Expr::Column(field)) if super::expr_is_constant(value) => {
            equality.fields.insert(field.to_ascii_lowercase());
            Some(())
        }
        (expr, value)
            if super::expr_has_column(expr)
                && !matches!(expr, Expr::Column(_))
                && super::expr_is_constant(value) =>
        {
            equality
                .expressions
                .insert(serde_json::to_string(expr).ok()?);
            Some(())
        }
        (value, expr)
            if super::expr_has_column(expr)
                && !matches!(expr, Expr::Column(_))
                && super::expr_is_constant(value) =>
        {
            equality
                .expressions
                .insert(serde_json::to_string(expr).ok()?);
            Some(())
        }
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct OrderShape {
    order_columns_used: usize,
    order_by_row_id: bool,
    reverse: bool,
    order_satisfied: bool,
}

fn order_shape(
    plan: &LogicalPlan,
    fields: &[String],
    equality_prefix_len: usize,
) -> Option<OrderShape> {
    if plan.order.is_empty() {
        return Some(OrderShape::default());
    }

    if plan.order.iter().any(|order| order.nulls.is_some()) {
        return None;
    }

    let order_terms = plan
        .order
        .iter()
        .map(|order| match &order.expr {
            Expr::Column(column) => Some((
                column.clone(),
                matches!(order.direction, SortDirection::Desc),
            )),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    let effective_order = order_terms
        .into_iter()
        .filter(|(column, _)| {
            !fields[..equality_prefix_len]
                .iter()
                .any(|field| field.eq_ignore_ascii_case(column))
        })
        .collect::<Vec<_>>();

    if effective_order.is_empty() {
        return Some(OrderShape {
            order_columns_used: 0,
            order_by_row_id: false,
            reverse: false,
            order_satisfied: true,
        });
    }

    let reverse = effective_order[0].1;
    if equality_prefix_len == fields.len()
        && effective_order.len() == 1
        && super::is_row_id_column(&effective_order[0].0)
    {
        return Some(OrderShape {
            order_columns_used: 0,
            order_by_row_id: true,
            reverse,
            order_satisfied: true,
        });
    }

    let remaining = &fields[equality_prefix_len..];
    let matched = remaining
        .iter()
        .zip(effective_order.iter())
        .take_while(|(field, (column, _))| field.eq_ignore_ascii_case(column))
        .count();
    if matched == 0 {
        return None;
    }
    let order_satisfied = effective_order
        .iter()
        .all(|(_, direction)| *direction == reverse);

    if matched == effective_order.len() {
        return Some(OrderShape {
            order_columns_used: matched,
            order_by_row_id: false,
            reverse,
            order_satisfied,
        });
    }

    if matched + 1 == effective_order.len()
        && super::is_row_id_column(
            &effective_order
                .last()
                .expect("order columns contain trailing row id")
                .0,
        )
    {
        return Some(OrderShape {
            order_columns_used: matched,
            order_by_row_id: true,
            reverse,
            order_satisfied,
        });
    }

    None
}

fn filter_constraint_shapes(
    filter: Option<&Expr>,
) -> Option<BTreeMap<String, FieldConstraintShape>> {
    let mut constraints = BTreeMap::new();
    let Some(filter) = filter else {
        return Some(constraints);
    };
    collect_filter_constraint_shapes(filter, &mut constraints)?;
    Some(constraints)
}

fn collect_filter_constraint_shapes(
    expr: &Expr,
    constraints: &mut BTreeMap<String, FieldConstraintShape>,
) -> Option<()> {
    match expr {
        Expr::Binary {
            left,
            op: BinaryOp::And,
            right,
        } => {
            collect_filter_constraint_shapes(left, constraints)?;
            collect_filter_constraint_shapes(right, constraints)
        }
        Expr::Binary { left, op, right } => {
            let (field, normalized) = field_constraint_shape(left, op, right)?;
            let entry = constraints.entry(field).or_default();
            match normalized {
                BinaryOp::Eq => entry.equality = true,
                BinaryOp::Gt | BinaryOp::Gte => entry.lower = true,
                BinaryOp::Lt | BinaryOp::Lte => entry.upper = true,
                _ => return None,
            }
            Some(())
        }
        Expr::Between {
            expr,
            low,
            high,
            negated: false,
        } => {
            let Expr::Column(field) = expr.as_ref() else {
                return None;
            };
            if !super::expr_is_constant(low) || !super::expr_is_constant(high) {
                return None;
            }
            let entry = constraints.entry(field.to_ascii_lowercase()).or_default();
            entry.lower = true;
            entry.upper = true;
            Some(())
        }
        _ => None,
    }
}

fn field_constraint_shape<'a>(
    left: &'a Expr,
    op: &BinaryOp,
    right: &'a Expr,
) -> Option<(String, BinaryOp)> {
    match (left, right) {
        (Expr::Column(field), other) if super::expr_is_constant(other) => {
            Some((field.to_ascii_lowercase(), op.clone()))
        }
        (other, Expr::Column(field)) if super::expr_is_constant(other) => {
            Some((field.to_ascii_lowercase(), reverse_binary_op(op)?))
        }
        _ => None,
    }
}

fn reverse_binary_op(op: &BinaryOp) -> Option<BinaryOp> {
    match op {
        BinaryOp::Eq => Some(BinaryOp::Eq),
        BinaryOp::Gt => Some(BinaryOp::Lt),
        BinaryOp::Gte => Some(BinaryOp::Lte),
        BinaryOp::Lt => Some(BinaryOp::Gt),
        BinaryOp::Lte => Some(BinaryOp::Gte),
        _ => None,
    }
}
