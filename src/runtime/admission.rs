use std::sync::atomic::Ordering;
use std::sync::Arc;

use super::RuntimeState;

impl RuntimeState {
    /// Attempts to reserve one local query-worker permit.
    ///
    /// # Panics
    ///
    /// Panics if the runtime metrics mutex is poisoned.
    pub fn try_begin_running_query(self: &Arc<Self>) -> Option<super::RunningQueryGuard> {
        let limit = self.limits.max_query_workers.max(1) as u64;
        let mut active = self.active_query_permits.load(Ordering::SeqCst);
        loop {
            if active >= limit {
                let mut metrics = self.metrics.lock().expect("runtime metrics");
                metrics.runtime.query_admission_rejections += 1;
                return None;
            }
            match self.active_query_permits.compare_exchange_weak(
                active,
                active + 1,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => break,
                Err(observed) => active = observed,
            }
        }

        let schema_epoch = self.schema_epoch();
        self.schema_epochs.begin(schema_epoch);
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.runtime.running_queries += 1;
        metrics.runtime.query_admission_permits += 1;
        drop(metrics);
        Some(super::RunningQueryGuard {
            runtime: Arc::clone(self),
            schema_epoch,
            admission_tracked: true,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::RuntimeState;
    use crate::config::CassieRuntimeLimits;
    use std::sync::Arc;

    #[test]
    fn should_release_query_admission_permit_after_drop() {
        // Arrange
        let limits = CassieRuntimeLimits {
            max_query_workers: 1,
            ..CassieRuntimeLimits::default()
        };
        let runtime = Arc::new(RuntimeState::new(limits));

        // Act
        let first = runtime.try_begin_running_query();
        let rejected = runtime.try_begin_running_query();
        drop(first);
        let after_release = runtime.try_begin_running_query();
        let metrics = runtime.snapshot();

        // Assert
        assert!(rejected.is_none());
        assert!(after_release.is_some());
        assert_eq!(metrics.runtime.query_admission_rejections, 1);
        assert_eq!(metrics.runtime.query_admission_permits, 2);
    }
}
