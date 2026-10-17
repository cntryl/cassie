//! Admission for the unpublished logical copy and every expanded expression.
use crate::app::CassieError;
use crate::executor::retained_memory::add;
use crate::planner::LogicalPlan;
use crate::runtime::QueryExecutionControls;
use crate::sql::parse_analysis as ast;
use std::mem::size_of;

pub(super) fn plan(
    plan: &LogicalPlan,
    controls: &QueryExecutionControls,
) -> Result<usize, CassieError> {
    controls.check_interruption()?;
    let mut bytes = add(size_of::<LogicalPlan>(), plan.collection.len())?;
    bytes = add(bytes, ast::source_clone_bytes(&plan.source, controls)?)?;
    for cte in &plan.ctes {
        bytes = add(bytes, ast::cte_clone_bytes(cte, controls)?)?;
    }
    for item in &plan.projection {
        bytes = add(bytes, ast::item_clone_bytes(item, 0, controls)?)?;
    }
    for expr in plan
        .group_by
        .iter()
        .chain(&plan.distinct_on)
        .chain(plan.filter.iter())
        .chain(plan.having.iter())
        .chain(plan.limit.iter())
        .chain(plan.offset.iter())
    {
        bytes = add(bytes, ast::expression_clone_bytes(expr, 0, controls)?)?;
    }
    for order in &plan.order {
        bytes = add(bytes, size_of::<crate::sql::ast::OrderExpr>())?;
        bytes = add(
            bytes,
            ast::expression_clone_bytes(&order.expr, 0, controls)?,
        )?;
    }
    if let Some(set) = &plan.set {
        bytes = add(bytes, size_of::<crate::sql::ast::SelectSet>())?;
        bytes = add(bytes, ast::select_clone_bytes(&set.right, controls)?)?;
    }
    Ok(bytes)
}
