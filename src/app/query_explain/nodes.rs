use crate::app::{Cassie, CassieError};
use crate::planner::physical::PhysicalPlan;

use super::allocation::PlanAllocation;
use super::{QueryPlanMetric, QueryPlanNode};

pub(super) fn plan_nodes(
    cassie: &Cassie,
    physical: &PhysicalPlan,
    allocation: &mut PlanAllocation<'_>,
) -> Result<Vec<QueryPlanNode>, CassieError> {
    let has_join = physical.join.strategy.is_some() || !physical.join.keys.is_empty();
    let has_aggregate = physical.aggregate.parallel_candidate || physical.aggregate.acceleration;
    let count = 2
        + usize::from(has_join)
        + usize::from(has_aggregate)
        + usize::from(physical.top_k.enabled);
    let mut nodes = allocation.vector(count)?;
    nodes.push(read_node(cassie, physical, allocation)?);
    if has_join {
        nodes.push(join_node(physical, allocation)?);
    }
    if has_aggregate {
        nodes.push(aggregate_node(physical, allocation)?);
    }
    if physical.top_k.enabled {
        nodes.push(top_k_node(physical, allocation)?);
    }
    nodes.push(project_node(physical, allocation)?);
    Ok(nodes)
}

fn metric(
    label: &str,
    value: String,
    allocation: &mut PlanAllocation<'_>,
) -> Result<QueryPlanMetric, CassieError> {
    Ok(QueryPlanMetric {
        label: allocation.text(label)?,
        value,
        unit: None,
    })
}

fn read_node(
    cassie: &Cassie,
    physical: &PhysicalPlan,
    allocation: &mut PlanAllocation<'_>,
) -> Result<QueryPlanNode, CassieError> {
    let count = usize::from(physical.read.selected_index.is_some())
        + usize::from(physical.read.predicate_pushdown)
        + usize::from(!physical.read.projected_scan_fields.is_empty())
        + usize::from(physical.read.covered_index)
        + usize::from(physical.read.column_batch_index.is_some());
    let mut badges = allocation.vector(count)?;
    if let Some(index) = &physical.read.selected_index {
        badges.push(allocation.format(format_args!("index:{index}"))?);
    }
    if physical.read.predicate_pushdown {
        badges.push(allocation.text("predicate pushdown")?);
    }
    if !physical.read.projected_scan_fields.is_empty() {
        badges.push(allocation.text("projection pruning")?);
    }
    if physical.read.covered_index {
        badges.push(allocation.text("covered index")?);
    }
    if let Some(index) = &physical.read.column_batch_index {
        badges.push(allocation.format(format_args!("column batch:{index}"))?);
    }
    let mut metrics = allocation.vector(3)?;
    metrics.push(metric(
        "estimated rows",
        allocation.format(format_args!("{}", super::selected_estimated_rows(physical)))?,
        allocation,
    )?);
    metrics.push(metric(
        "selected cost",
        allocation.format(format_args!("{}", super::selected_cost(physical)))?,
        allocation,
    )?);
    metrics.push(metric(
        "storage",
        allocation.text(super::storage_mode(cassie, physical))?,
        allocation,
    )?);
    let label = match physical.read.selected_index.as_deref() {
        Some(index) => allocation.format(format_args!("Read with {index}"))?,
        None => allocation.text("Read collection")?,
    };
    let status = if physical.read.fallback_reason.is_some() {
        "fallback"
    } else if physical.read.selected_index.is_some()
        || physical.read.predicate_pushdown
        || physical.read.covered_index
    {
        "optimized"
    } else {
        "baseline"
    };
    Ok(QueryPlanNode {
        id: allocation.text("read")?,
        label,
        kind: allocation.text("read")?,
        detail: allocation.format(format_args!(
            "{} via {}",
            physical.collection,
            physical.read.access_path.as_str()
        ))?,
        status: allocation.text(status)?,
        badges,
        metrics,
    })
}

fn join_node(
    physical: &PhysicalPlan,
    allocation: &mut PlanAllocation<'_>,
) -> Result<QueryPlanNode, CassieError> {
    let detail = if physical.join.keys.is_empty() {
        allocation.text("No join keys captured")?
    } else {
        allocation.join(physical.join.keys.iter().map(String::as_str), ", ")?
    };
    let mut badges = allocation.vector(1)?;
    badges.push(allocation.format(format_args!(
        "sort required:{}",
        physical.join.sort_required
    ))?);
    let mut metrics = allocation.vector(1)?;
    metrics.push(metric(
        "estimated rows",
        allocation.format(format_args!("{}", physical.estimates.join_rows))?,
        allocation,
    )?);
    Ok(QueryPlanNode {
        id: allocation.text("join")?,
        label: allocation.format(format_args!(
            "{} join",
            physical.join.strategy.as_deref().unwrap_or("runtime")
        ))?,
        kind: allocation.text("join")?,
        detail,
        status: allocation.text(if physical.join.fallback_reason.is_some() {
            "fallback"
        } else {
            "optimized"
        })?,
        badges,
        metrics,
    })
}

fn aggregate_node(
    physical: &PhysicalPlan,
    allocation: &mut PlanAllocation<'_>,
) -> Result<QueryPlanNode, CassieError> {
    let count = usize::from(physical.aggregate.parallel_candidate)
        + usize::from(physical.aggregate.acceleration);
    let mut badges = allocation.vector(count)?;
    if physical.aggregate.parallel_candidate {
        badges.push(allocation.text("parallel candidate")?);
    }
    if physical.aggregate.acceleration {
        badges.push(allocation.text("accelerated")?);
    }
    let mut metrics = allocation.vector(1)?;
    metrics.push(metric(
        "estimated rows",
        allocation.format(format_args!("{}", physical.estimates.aggregate_rows))?,
        allocation,
    )?);
    Ok(QueryPlanNode {
        id: allocation.text("aggregate")?,
        label: allocation.text("Aggregate")?,
        kind: allocation.text("aggregate")?,
        detail: allocation.text("Group and aggregate rows")?,
        status: allocation.text(if physical.aggregate.acceleration {
            "optimized"
        } else {
            "baseline"
        })?,
        badges,
        metrics,
    })
}

fn top_k_node(
    physical: &PhysicalPlan,
    allocation: &mut PlanAllocation<'_>,
) -> Result<QueryPlanNode, CassieError> {
    let limit = match physical.top_k.limit {
        Some(limit) => allocation.format(format_args!("{limit}"))?,
        None => allocation.text("none")?,
    };
    let mut badges = allocation.vector(1)?;
    badges.push(allocation.format(format_args!("limit:{limit}"))?);
    let mut metrics = allocation.vector(1)?;
    metrics.push(metric(
        "candidate budget",
        allocation.format(format_args!("{}", physical.top_k.limit.unwrap_or_default()))?,
        allocation,
    )?);
    Ok(QueryPlanNode {
        id: allocation.text("top_k")?,
        label: allocation.text("Top K")?,
        kind: allocation.text("top_k")?,
        detail: allocation.format(format_args!("{} ordering", physical.top_k.mode.as_str()))?,
        status: allocation.text(
            if matches!(
                physical.top_k.mode,
                crate::planner::physical::TopKMode::Storage
            ) {
                "optimized"
            } else {
                "baseline"
            },
        )?,
        badges,
        metrics,
    })
}

fn project_node(
    physical: &PhysicalPlan,
    allocation: &mut PlanAllocation<'_>,
) -> Result<QueryPlanNode, CassieError> {
    let fields = &physical.read.projected_scan_fields;
    let mut badges = allocation.vector(fields.len().max(1))?;
    if fields.is_empty() {
        badges.push(allocation.text("all fields")?);
    } else {
        for field in fields {
            badges.push(allocation.format(format_args!("field:{field}"))?);
        }
    }
    let scan_fields = if fields.is_empty() {
        allocation.text("all")?
    } else {
        allocation.join(fields.iter().map(String::as_str), ", ")?
    };
    let mut metrics = allocation.vector(1)?;
    metrics.push(metric("scan fields", scan_fields, allocation)?);
    Ok(QueryPlanNode {
        id: allocation.text("project")?,
        label: allocation.text("Project rows")?,
        kind: allocation.text("project")?,
        detail: allocation.text(physical.projection.shape.as_str())?,
        status: allocation.text("active")?,
        badges,
        metrics,
    })
}
