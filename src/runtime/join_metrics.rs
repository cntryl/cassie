use super::RuntimeState;

impl RuntimeState {
    pub(crate) fn record_join_execution(
        &self,
        strategy: &str,
        left_rows: usize,
        right_rows: usize,
        matched_rows: usize,
        output_rows: usize,
        fallback_reason: Option<&str>,
    ) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.joins.executions += 1;
        match strategy {
            "merge" => metrics.joins.merge_joins += 1,
            "vectorized" => metrics.joins.vectorized_joins += 1,
            _ => metrics.joins.scalar_joins += 1,
        }
        if fallback_reason.is_some() {
            metrics.joins.fallback_joins += 1;
        }
        metrics.joins.left_input_rows_total += left_rows as u64;
        metrics.joins.right_input_rows_total += right_rows as u64;
        metrics.joins.matched_rows_total += matched_rows as u64;
        metrics.joins.output_rows_total += output_rows as u64;
        metrics.joins.last_strategy = strategy.to_string();
        metrics.joins.last_fallback_reason = fallback_reason.unwrap_or("").to_string();
    }

    pub(crate) fn record_vectorized_join_execution(
        &self,
        left_rows: usize,
        right_rows: usize,
        matched_rows: usize,
        output_rows: usize,
        batch_size: usize,
        batches: usize,
    ) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.joins.executions += 1;
        metrics.joins.vectorized_joins += 1;
        metrics.joins.left_input_rows_total += left_rows as u64;
        metrics.joins.right_input_rows_total += right_rows as u64;
        metrics.joins.matched_rows_total += matched_rows as u64;
        metrics.joins.output_rows_total += output_rows as u64;
        metrics.joins.vectorized_batches_total += batches as u64;
        metrics.joins.vectorized_build_rows_total += right_rows as u64;
        metrics.joins.vectorized_probe_rows_total += left_rows as u64;
        metrics.joins.last_vectorized_batch_size = batch_size as u64;
        metrics.joins.last_strategy = "vectorized".to_string();
        metrics.joins.last_fallback_reason.clear();
    }

    pub(crate) fn record_vectorized_join_execution_with_roles(
        &self,
        input_rows: VectorizedJoinInputRows,
        matched_rows: usize,
        output_rows: usize,
        batch_size: usize,
        batches: usize,
    ) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.joins.executions += 1;
        metrics.joins.vectorized_joins += 1;
        metrics.joins.left_input_rows_total += input_rows.left as u64;
        metrics.joins.right_input_rows_total += input_rows.right as u64;
        metrics.joins.matched_rows_total += matched_rows as u64;
        metrics.joins.output_rows_total += output_rows as u64;
        metrics.joins.vectorized_batches_total += batches as u64;
        metrics.joins.vectorized_build_rows_total += input_rows.build as u64;
        metrics.joins.vectorized_probe_rows_total += input_rows.probe as u64;
        metrics.joins.last_vectorized_batch_size = batch_size as u64;
        metrics.joins.last_strategy = "vectorized".to_string();
        metrics.joins.last_fallback_reason.clear();
    }

    pub(crate) fn record_vectorized_join_fallback(
        &self,
        reason: &str,
        batch_size: usize,
        spill: bool,
    ) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.joins.vectorized_fallbacks += 1;
        metrics.joins.fallback_joins += 1;
        if spill {
            metrics.joins.vectorized_spill_fallbacks += 1;
        }
        metrics.joins.last_vectorized_batch_size = batch_size as u64;
        metrics.joins.last_vectorized_fallback_reason = reason.to_string();
    }
}

#[derive(Clone, Copy)]
pub(crate) struct VectorizedJoinInputRows {
    pub(crate) left: usize,
    pub(crate) right: usize,
    pub(crate) build: usize,
    pub(crate) probe: usize,
}
