use super::*;

#[derive(Debug, Clone, Copy)]
pub struct QueryExecutionControls {
    pub deadline: Option<Instant>,
    pub max_result_rows: usize,
    pub temp_spill_budget_bytes: usize,
    pub cte_recursion_depth: usize,
}

impl QueryExecutionControls {
    pub fn from_limits(limits: &CassieRuntimeLimits, started_at: Instant) -> Self {
        let deadline = if limits.query_timeout_ms == 0 {
            Some(started_at)
        } else {
            started_at
                .checked_add(Duration::from_millis(limits.query_timeout_ms))
                .or(Some(started_at))
        };

        Self {
            deadline,
            max_result_rows: limits.max_result_rows,
            temp_spill_budget_bytes: limits.temp_spill_budget_bytes,
            cte_recursion_depth: limits.cte_recursion_depth,
        }
    }

    pub fn is_timed_out(&self) -> bool {
        self.deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
    }
}
