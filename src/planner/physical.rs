use crate::planner::logical::LogicalPlan;
use crate::sql::ast::{QuerySource, SelectItem};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Operator {
    Scan,
    Filter,
    Project,
    Sort,
    Limit,
    Offset,
    VectorSearch,
    FullTextSearch,
    Join,
    Aggregate,
    Distinct,
    SetOperation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhysicalPlan {
    pub collection: String,
    pub operators: Vec<Operator>,
    pub logical: LogicalPlan,
}

pub fn build(plan: LogicalPlan) -> PhysicalPlan {
    if plan.command.is_some() {
        return PhysicalPlan {
            collection: plan.collection.clone(),
            operators: Vec::new(),
            logical: plan,
        };
    }

    let mut operators = vec![Operator::Scan];
    if source_contains_join(&plan.source) {
        operators.push(Operator::Join);
    }
    if plan.filter.is_some() {
        operators.push(Operator::Filter);
    }
    if plan_uses_aggregate(&plan) {
        operators.push(Operator::Aggregate);
    }
    if plan.distinct || !plan.distinct_on.is_empty() {
        operators.push(Operator::Distinct);
    }
    if plan.set.is_some() {
        operators.push(Operator::SetOperation);
    }
    if !plan.order.is_empty() {
        operators.push(Operator::Sort);
    }
    if !plan.projection.is_empty() {
        operators.push(Operator::Project);
    }
    if plan.offset.is_some() {
        operators.push(Operator::Offset);
    }
    if plan.limit.is_some() {
        operators.push(Operator::Limit);
    }
    PhysicalPlan {
        collection: plan.collection.clone(),
        operators,
        logical: plan,
    }
}

fn source_contains_join(source: &QuerySource) -> bool {
    match source {
        QuerySource::Join { .. } => true,
        QuerySource::Subquery { select, .. } => source_contains_join(&select.source),
        QuerySource::Collection(_) | QuerySource::Cte(_) | QuerySource::SingleRow => false,
    }
}

fn plan_uses_aggregate(plan: &LogicalPlan) -> bool {
    !plan.group_by.is_empty()
        || plan.having.is_some()
        || plan.projection.iter().any(|item| match item {
            SelectItem::Function { function, .. } => {
                crate::sql::functions::is_aggregate_function(&function.name)
            }
            SelectItem::Wildcard
            | SelectItem::Column { .. }
            | SelectItem::Expr { .. }
            | SelectItem::WindowFunction { .. } => false,
        })
}
