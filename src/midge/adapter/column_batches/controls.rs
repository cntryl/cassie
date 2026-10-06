use super::{CassieError, Midge, QueryExecutionControls};

pub(super) fn check_column_batch_controls(
    midge: &Midge,
    controls: &QueryExecutionControls,
) -> Result<(), CassieError> {
    if controls.is_cancelled() {
        return Err(CassieError::QueryCancelled);
    }
    if controls.is_timed_out() {
        return Err(CassieError::DeadlineExceeded);
    }
    midge.record_query_scan_entry();
    if super::super::query_scan_control::should_cancel_controlled_query_scan() {
        return Err(CassieError::QueryCancelled);
    }
    Ok(())
}
