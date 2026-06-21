use super::*;

pub(super) fn plan_supports_aggregate_acceleration(
    plan: &LogicalPlan,
    indexes: &[IndexMeta],
) -> bool {
    let QuerySource::Collection(collection) = &plan.source else {
        return false;
    };
    if plan.command.is_some()
        || !plan.ctes.is_empty()
        || plan.filter.is_some()
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
    let (fields, count_star_only) = aggregate_acceleration_fields(plan);
    if fields.is_empty() && !count_star_only {
        return false;
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
