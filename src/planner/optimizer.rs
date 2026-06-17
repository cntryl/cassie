use crate::planner::logical::LogicalPlan;

pub fn optimize(mut plan: LogicalPlan) -> LogicalPlan {
    if plan.offset.is_none() {
        plan.offset = Some(0);
    }
    plan
}
