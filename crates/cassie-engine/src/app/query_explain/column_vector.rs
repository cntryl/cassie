//! The `column_vector` EXPLAIN tokens (#1358).

use super::allocation::PlanAllocation;
use crate::app::{Cassie, CassieError, CassieSession};
use crate::types::Value;

/// `column_vector=eligible|fallback|not_applicable column_vector_reason=<label>`.
///
/// The decision is the one the typed scan dispatch applies, so a plan that
/// says `eligible` reads vector batches and one that says `fallback` reads row
/// documents for the stated reason.
pub(super) fn section(
    cassie: &Cassie,
    physical: &crate::planner::physical::PhysicalPlan,
    runtime: (Option<&CassieSession>, &[Value]),
    allocation: &mut PlanAllocation<'_>,
) -> Result<String, CassieError> {
    let eligibility = crate::executor::column_vector_eligibility(
        cassie,
        &physical.logical,
        runtime.1,
        allocation.controls(),
    )
    .map_err(CassieError::from)?;
    allocation.format(format_args!(
        "column_vector={} column_vector_reason={}",
        eligibility.token(),
        eligibility.reason()
    ))
}
