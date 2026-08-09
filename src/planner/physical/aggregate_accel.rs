use super::{
    is_row_id_column, BTreeSet, BinaryOp, Expr, IndexKind, IndexMeta, LogicalPlan, QuerySource,
    SelectItem,
};

pub(super) fn plan_supports_aggregate_acceleration(
    plan: &LogicalPlan,
    indexes: &[IndexMeta],
) -> bool {
    let QuerySource::Collection(collection) = &plan.source else {
        return false;
    };
    if plan.command.is_some()
        || !plan.ctes.is_empty()
        || !plan.group_by.is_empty()
        || plan.having.is_some()
        || !plan.order.is_empty()
        || plan.limit.is_some()
        || plan.offset.unwrap_or(0) != 0
        || plan.distinct
        || !plan.distinct_on.is_empty()
        || plan.set.is_some()
    {
        return false;
    }
    let (mut fields, count_star_only) = aggregate_acceleration_fields(plan);
    if fields.is_empty() && !count_star_only {
        return false;
    }
    if let Some(filter) = plan.filter.as_ref() {
        let Some(filter_fields) = encoded_filter_fields(filter) else {
            return false;
        };
        fields.extend(filter_fields);
    }
    indexes
        .iter()
        .filter(|index| index.collection == *collection && index.kind == IndexKind::Column)
        .any(|index| {
            let available = index
                .normalized_fields()
                .into_iter()
                .map(|field| field.to_ascii_lowercase())
                .collect::<BTreeSet<_>>();
            fields.iter().all(|field| available.contains(field))
        })
}

fn encoded_filter_fields(expr: &Expr) -> Option<BTreeSet<String>> {
    match expr {
        Expr::Binary {
            left,
            op: BinaryOp::And,
            right,
        } => {
            let mut fields = encoded_filter_fields(left)?;
            fields.extend(encoded_filter_fields(right)?);
            Some(fields)
        }
        Expr::Binary {
            left,
            op: BinaryOp::Eq | BinaryOp::Lt | BinaryOp::Lte | BinaryOp::Gt | BinaryOp::Gte,
            right,
        } => encoded_binary_filter_field(left, right),
        Expr::Between {
            expr,
            low,
            high,
            negated: false,
        } if encoded_literal(low) && encoded_literal(high) => {
            let Expr::Column(field) = expr.as_ref() else {
                return None;
            };
            (!is_row_id_column(field)).then(|| BTreeSet::from([field.to_ascii_lowercase()]))
        }
        Expr::IsNull { expr, .. } => {
            let Expr::Column(field) = expr.as_ref() else {
                return None;
            };
            (!is_row_id_column(field)).then(|| BTreeSet::from([field.to_ascii_lowercase()]))
        }
        _ => None,
    }
}

fn encoded_binary_filter_field(left: &Expr, right: &Expr) -> Option<BTreeSet<String>> {
    match (left, right) {
        (Expr::Column(field), literal) | (literal, Expr::Column(field))
            if encoded_literal(literal) =>
        {
            (!is_row_id_column(field)).then(|| BTreeSet::from([field.to_ascii_lowercase()]))
        }
        _ => None,
    }
}

fn encoded_literal(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::StringLiteral(_)
            | Expr::BoolLiteral(_)
            | Expr::NumberLiteral(_)
            | Expr::IntegerLiteral(_)
    )
}

fn aggregate_acceleration_fields(plan: &LogicalPlan) -> (BTreeSet<String>, bool) {
    let mut fields = BTreeSet::new();
    let mut count_star_only = false;
    for item in &plan.projection {
        let SelectItem::Function { function, .. } = item else {
            return (BTreeSet::new(), false);
        };
        let name = function.name.to_ascii_lowercase();
        if !matches!(name.as_str(), "count" | "sum" | "avg" | "min" | "max") {
            return (BTreeSet::new(), false);
        }
        match function.args.as_slice() {
            [Expr::Column(column)] if column == "*" && name == "count" => {
                count_star_only = true;
            }
            [Expr::Column(column)] if column != "*" => {
                fields.insert(column.to_ascii_lowercase());
            }
            _ => return (BTreeSet::new(), false),
        }
    }
    let only_count_star = count_star_only && fields.is_empty();
    (fields, only_count_star)
}
