use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use super::RuntimeState;

#[derive(Debug, Default)]
pub(super) struct SchemaEpochTracker {
    active: Mutex<BTreeMap<u64, u64>>,
}

pub struct RunningQueryGuard {
    runtime: Arc<RuntimeState>,
    schema_epoch: u64,
}

impl Drop for RunningQueryGuard {
    fn drop(&mut self) {
        self.runtime.finish_running_query(self.schema_epoch);
    }
}

impl SchemaEpochTracker {
    fn begin(&self, schema_epoch: u64) {
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
        }
    }

    fn finish_running_query(&self, schema_epoch: u64) {
        self.schema_epochs.finish(schema_epoch);
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.runtime.running_queries = metrics.runtime.running_queries.saturating_sub(1);
    }

    pub fn has_active_schema_epoch_at_or_before(&self, schema_epoch: u64) -> bool {
        self.schema_epochs.has_active_at_or_before(schema_epoch)
    }
}
