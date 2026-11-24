//! Native request owners for adapter fixtures.

use std::sync::Arc;
use std::time::{Duration, Instant};

use cassie_core::request::{Interruption, RequestCancellation, RequestMemory};

use super::{current_query_scan_control_scope, QueryScanControlScope, StorageRequestContext};

const DEFAULT_BUDGET: usize = 10 * 1024 * 1024;
const HANG_GUARD: Duration = Duration::from_secs(30);

pub(super) struct TestRequest {
    pub(super) deadline: Option<Instant>,
    cancellation: RequestCancellation,
    memory: RequestMemory,
    scan_control: Option<Arc<QueryScanControlScope>>,
}

impl Default for TestRequest {
    fn default() -> Self {
        Self::with_budget(DEFAULT_BUDGET)
    }
}

impl TestRequest {
    pub(super) fn with_budget(budget: usize) -> Self {
        let mut owner = Self::without_deadline(budget);
        owner.deadline = Instant::now().checked_add(HANG_GUARD);
        owner
    }

    pub(super) fn without_deadline(budget: usize) -> Self {
        Self {
            deadline: None,
            cancellation: RequestCancellation::new(),
            memory: RequestMemory::new(budget),
            scan_control: current_query_scan_control_scope(),
        }
    }

    pub(super) fn context(&self) -> StorageRequestContext<'_> {
        StorageRequestContext::new(
            None,
            Interruption::new(self.deadline, &self.cancellation),
            &self.memory,
        )
        .with_scan_control(self.scan_control.as_deref())
    }

    pub(super) fn current_query_memory_bytes(&self) -> usize {
        self.memory.current_bytes()
    }
}
