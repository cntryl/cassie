use super::{Arc, Ordering, RuntimeState};

pub(crate) struct OperatorWorkerGuard {
    runtime: Arc<RuntimeState>,
    workers: u64,
}

impl OperatorWorkerGuard {
    #[must_use]
    pub(crate) fn workers(&self) -> usize {
        usize::try_from(self.workers).unwrap_or(usize::MAX)
    }
}

impl Drop for OperatorWorkerGuard {
    fn drop(&mut self) {
        self.runtime
            .active_operator_workers
            .fetch_sub(self.workers, Ordering::AcqRel);
    }
}

impl RuntimeState {
    pub(crate) fn try_acquire_operator_workers(
        self: &Arc<Self>,
        requested: usize,
    ) -> Option<OperatorWorkerGuard> {
        let limit = self
            .limits
            .parallel_scan_workers
            .max(self.limits.parallel_scoring_workers)
            .max(self.limits.parallel_aggregation_workers)
            .max(1) as u64;
        let requested = u64::try_from(requested).unwrap_or(u64::MAX).min(limit);
        let mut active = self.active_operator_workers.load(Ordering::Acquire);
        loop {
            let available = limit.saturating_sub(active);
            let workers = requested.min(available);
            if workers < 2 {
                return None;
            }
            match self.active_operator_workers.compare_exchange_weak(
                active,
                active + workers,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    return Some(OperatorWorkerGuard {
                        runtime: Arc::clone(self),
                        workers,
                    });
                }
                Err(actual) => active = actual,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CassieRuntimeLimits;

    #[test]
    fn should_prevent_operator_worker_permit_oversubscription() {
        // Arrange
        let limits = CassieRuntimeLimits {
            parallel_scan_workers: 2,
            ..CassieRuntimeLimits::default()
        };
        let runtime = Arc::new(RuntimeState::new(limits));
        let first = runtime
            .try_acquire_operator_workers(2)
            .expect("first operator permits");

        // Act
        let rejected = runtime.try_acquire_operator_workers(2);
        let active = runtime.snapshot().runtime.active_operator_workers;
        drop(first);
        let after_release = runtime
            .try_acquire_operator_workers(2)
            .expect("released permits");

        // Assert
        assert!(rejected.is_none());
        assert_eq!(active, 2);
        assert_eq!(after_release.workers(), 2);
        drop(after_release);
        assert_eq!(runtime.snapshot().runtime.active_operator_workers, 0);
    }
}
