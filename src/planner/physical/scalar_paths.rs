use super::*;
use crate::sql::ast::SortDirection;
use std::collections::BTreeMap;

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
    pub equality_prefix_len: usize,
    pub range_field_index: Option<usize>,
    pub order_columns_used: usize,
    pub order_by_row_id: bool,
    pub reverse: bool,
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
        || !index.expressions.is_empty()
    {
        return None;
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
        });
    }

    if order_shape.order_columns_used > 0 && plan.limit.is_some() {
        return Some(ScalarIndexPlanShape {
            path: ScalarIndexPlanPath::OrderedBoundedScan,
            equality_prefix_len,
            range_field_index: None,
            order_columns_used: order_shape.order_columns_used,
            order_by_row_id: order_shape.order_by_row_id,
            reverse: order_shape.reverse,
        });
    }

    None
}

#[derive(Debug, Clone, Copy, Default)]
struct OrderShape {
    order_columns_used: usize,
    order_by_row_id: bool,
    reverse: bool,
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

    let direction = plan.order.first()?.direction.clone();
    let reverse = matches!(direction, SortDirection::Desc);
    if plan
        .order
        .iter()
        .any(|order| matches!(order.direction, SortDirection::Desc) != reverse)
    {
        return None;
    }

    let order_columns = plan
        .order
        .iter()
        .map(|order| match &order.expr {
            Expr::Column(column) => Some(column.clone()),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;

    if equality_prefix_len == fields.len()
        && order_columns.len() == 1
        && super::is_row_id_column(&order_columns[0])
    {
        return Some(OrderShape {
            order_columns_used: 0,
            order_by_row_id: true,
            reverse,
        });
    }

    if !order_columns.is_empty()
        && order_columns.iter().all(|column| {
            fields[..equality_prefix_len]
                .iter()
                .any(|field| field.eq_ignore_ascii_case(column))
        })
    {
        return Some(OrderShape {
            order_columns_used: 0,
            order_by_row_id: false,
            reverse,
        });
    }

    let remaining = &fields[equality_prefix_len..];
    let matched = remaining
        .iter()
        .zip(order_columns.iter())
        .take_while(|(field, column)| field.eq_ignore_ascii_case(column))
        .count();
    if matched == 0 {
        return None;
    }

    if matched == order_columns.len() {
        return Some(OrderShape {
            order_columns_used: matched,
            order_by_row_id: false,
            reverse,
        });
    }

    if matched + 1 == order_columns.len()
        && super::is_row_id_column(
            order_columns
                .last()
                .expect("order columns contain trailing row id"),
        )
    {
        return Some(OrderShape {
            order_columns_used: matched,
            order_by_row_id: true,
            reverse,
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
