use super::*;

impl RuntimeState {
    pub fn record_time_series_scan(
        &self,
        index: impl Into<String>,
        rows: usize,
        buckets_scanned: usize,
        buckets_skipped: usize,
    ) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.time_series.scans += 1;
        metrics.time_series.rows += rows as u64;
        metrics.time_series.buckets_scanned += buckets_scanned as u64;
        metrics.time_series.buckets_skipped += buckets_skipped as u64;
        metrics.time_series.last_index = index.into();
    }

    pub fn record_time_series_fallback(&self, reason: impl Into<String>) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.time_series.fallback_scans += 1;
        metrics.time_series.last_fallback_reason = reason.into();
    }
}
