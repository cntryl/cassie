//! Row-selection counters of single-table UPDATE and DELETE statements.

use super::RuntimeState;

impl RuntimeState {
    /// Records that an UPDATE or DELETE fetched only keyed candidate rows.
    ///
    /// # Panics
    ///
    /// Panics if the runtime metrics lock is poisoned.
    pub fn record_dml_keyed_row_lookup(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.read_paths.dml_keyed_row_lookups_total += 1;
    }

    /// Records that an UPDATE or DELETE scanned the whole target table.
    ///
    /// # Panics
    ///
    /// Panics if the runtime metrics lock is poisoned.
    pub fn record_dml_full_scan(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.read_paths.dml_full_scan_total += 1;
    }
}
