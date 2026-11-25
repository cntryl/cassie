use std::collections::BTreeMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use super::RuntimeState;

#[derive(Debug, Default)]
pub(super) struct SchemaEpochTracker {
    active: Mutex<BTreeMap<u64, u64>>,
}

pub struct RunningQueryGuard {
    pub(super) runtime: Arc<RuntimeState>,
    pub(super) schema_epoch: u64,
    pub(super) admission_tracked: bool,
}

impl Drop for RunningQueryGuard {
    fn drop(&mut self) {
        self.runtime
            .finish_running_query(self.schema_epoch, self.admission_tracked);
    }
}

impl SchemaEpochTracker {
    pub(super) fn begin(&self, schema_epoch: u64) {
        let mut active = self.active.lock().expect("schema epoch tracker");
        *active.entry(schema_epoch).or_insert(0) += 1;
    }

    fn finish(&self, schema_epoch: u64) {
        let mut active = self.active.lock().expect("schema epoch tracker");
        let Some(count) = active.get_mut(&schema_epoch) else {
            return;
        };
        *count = count.saturating_sub(1);
        if *count == 0 {
            active.remove(&schema_epoch);
        }
    }

    fn has_active_at_or_before(&self, schema_epoch: u64) -> bool {
        self.active
            .lock()
            .expect("schema epoch tracker")
            .range(..=schema_epoch)
            .any(|(_, count)| *count > 0)
    }
}

impl RuntimeState {
    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn begin_running_query(self: &Arc<Self>) -> RunningQueryGuard {
        let schema_epoch = self.schema_epoch();
        self.schema_epochs.begin(schema_epoch);
        {
            let mut metrics = self.metrics.lock().expect("runtime metrics");
            metrics.runtime.running_queries += 1;
        }

        RunningQueryGuard {
            runtime: Arc::clone(self),
            schema_epoch,
            admission_tracked: false,
        }
    }

    fn finish_running_query(&self, schema_epoch: u64, admission_tracked: bool) {
        self.schema_epochs.finish(schema_epoch);
        if admission_tracked {
            self.active_query_permits.fetch_sub(1, Ordering::SeqCst);
        }
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.runtime.running_queries = metrics.runtime.running_queries.saturating_sub(1);
    }

    pub fn has_active_schema_epoch_at_or_before(&self, schema_epoch: u64) -> bool {
        self.schema_epochs.has_active_at_or_before(schema_epoch)
    }
}
