use super::*;

impl RuntimeState {
    pub fn record_retention_enforcement(
        &self,
        policy: impl Into<String>,
        deleted: u64,
        skipped: u64,
    ) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.retention.enforcements += 1;
        metrics.retention.deleted_rows += deleted;
        metrics.retention.skipped_rows += skipped;
        metrics.retention.last_policy = policy.into();
    }

    pub fn record_retention_error(&self, policy: impl Into<String>, error: impl Into<String>) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.retention.errors += 1;
        metrics.retention.last_policy = policy.into();
        metrics.retention.last_error = error.into();
    }
}
