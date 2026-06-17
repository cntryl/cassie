use crate::planner::logical::LogicalPlan;

#[derive(Debug, Clone)]
pub enum Operator {
    Scan,
    Filter,
    Project,
    Sort,
    Limit,
    Offset,
    VectorSearch,
    FullTextSearch,
}

#[derive(Debug, Clone)]
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
    if plan.filter.is_some() {
        operators.push(Operator::Filter);
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
