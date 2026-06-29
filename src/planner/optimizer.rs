use crate::planner::logical::LogicalPlan;

#[must_use]
pub fn optimize(mut plan: LogicalPlan) -> LogicalPlan {
    if plan.offset.is_none() {
        plan.offset = Some(0);
    }
    plan
}
