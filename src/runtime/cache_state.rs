use std::collections::{HashMap, VecDeque};
use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::planner::physical::PhysicalPlan;

use super::{
    current_time_millis, touch, ExecutionResultCacheKey, L1PlanEntry, L1PlanHit, PlanCacheKey,
    RuntimeState,
};

#[cfg(test)]
#[path = "cache_ownership_tests.rs"]
mod ownership_tests;
#[cfg(test)]
#[path = "cache_state_tests.rs"]
mod tests;

#[derive(Debug, Default)]
pub(super) struct ExecutionResultCacheState {
    // Retire the deque's duplicate keys before entry-held key leases.
    pub(super) order: VecDeque<ExecutionResultCacheKey>,
    pub(super) entries: HashMap<ExecutionResultCacheKey, ExecutionResultCacheEntry>,
    pub(super) bytes: usize,
    pub(super) map_memory: Option<super::QueryMemoryReservation>,
    pub(super) order_memory: Option<super::QueryMemoryReservation>,
}

#[derive(Debug)]
pub(super) struct ExecutionResultCacheEntry {
    pub(super) result: crate::executor::QueryResult,
    pub(super) bytes: usize,
    pub(super) key_memory: super::QueryMemoryReservation,
}

impl RuntimeState {
    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn plan_cache_lookup(&self, key: &PlanCacheKey) -> Option<L1PlanHit> {
        let mut cache = self.plan_cache.lock().expect("plan cache");
        if let Some(plan) = cache.entries.get(key).cloned() {
            touch(&mut cache.order, key);
            drop(cache);
            self.record_query_cache_l1_hit();
            return Some(L1PlanHit {
                plan: plan.plan,
                durable: plan.durable,
                candidate_expires_at_ms: plan.candidate_expires_at_ms,
            });
        }

        drop(cache);
        self.record_query_cache_l1_miss();
        None
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn plan_cache_store(&self, key: PlanCacheKey, plan: Arc<PhysicalPlan>, durable: bool) {
        let max_entries = self.limits.plan_cache_entries.max(1);
        let mut cache = self.plan_cache.lock().expect("plan cache");
        let mut evictions = 0;
        let entry = L1PlanEntry {
            plan,
            durable,
            candidate_expires_at_ms: None,
        };

        if cache.entries.contains_key(&key) {
            cache.entries.insert(key.clone(), entry);
            touch(&mut cache.order, &key);
        } else {
            if cache.entries.len() >= max_entries {
                if let Some(oldest) = cache.order.pop_front() {
                    if cache.entries.remove(&oldest).is_some() {
                        evictions += 1;
                    }
                }
            }

            cache.entries.insert(key.clone(), entry);
            cache.order.push_back(key);
        }

        drop(cache);
        self.record_plan_cache_eviction(evictions);
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn mark_plan_cache_entry_durable(&self, key: &PlanCacheKey) {
        let mut cache = self.plan_cache.lock().expect("plan cache");
        if let Some(entry) = cache.entries.get_mut(key) {
            entry.durable = true;
            entry.candidate_expires_at_ms = None;
        }
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn mark_plan_cache_entry_candidate_pending(&self, key: &PlanCacheKey, ttl_seconds: u64) {
        if ttl_seconds == 0 {
            return;
        }

        let expires_at_ms = current_time_millis().saturating_add(ttl_seconds.saturating_mul(1000));
        let mut cache = self.plan_cache.lock().expect("plan cache");
        if let Some(entry) = cache.entries.get_mut(key) {
            if !entry.durable {
                entry.candidate_expires_at_ms = Some(expires_at_ms);
            }
        }
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn invalidate_plan_cache(&self) {
        let mut cache = self.plan_cache.lock().expect("plan cache");
        cache.entries.clear();
        cache.order.clear();
        drop(cache);
        self.fulltext_index_options
            .lock()
            .expect("fulltext index options")
            .clear();
        self.record_plan_cache_invalidation();
    }

    pub fn data_epoch(&self) -> u64 {
        self.data_epoch.load(Ordering::SeqCst)
    }

    pub fn bump_data_epoch(&self) {
        let epoch = self
            .data_epoch
            .fetch_add(1, Ordering::SeqCst)
            .wrapping_add(1);
        self.data_epoch.store(epoch, Ordering::SeqCst);
        self.invalidate_execution_result_cache();
    }

    /// Installs the durable epoch loaded from storage and invalidates stale cache entries.
    pub fn set_data_epoch(&self, epoch: u64) {
        self.data_epoch.store(epoch, Ordering::SeqCst);
        self.invalidate_execution_result_cache();
    }

    pub fn index_feedback_epoch(&self) -> u64 {
        self.index_feedback_epoch.load(Ordering::SeqCst)
    }

    pub(super) fn bump_index_feedback_epoch(&self) {
        let epoch = self
            .index_feedback_epoch
            .fetch_add(1, Ordering::SeqCst)
            .wrapping_add(1);
        self.index_feedback_epoch.store(epoch, Ordering::SeqCst);
    }

    /// # Panics
    ///
    /// Panics if the runtime metrics lock is poisoned.
    pub fn record_execution_result_cache_hit(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.execution_result_cache.hits += 1;
    }

    /// # Panics
    ///
    /// Panics if the runtime metrics lock is poisoned.
    pub fn record_execution_result_cache_miss(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.execution_result_cache.misses += 1;
    }

    /// # Panics
    ///
    /// Panics if the runtime metrics lock is poisoned.
    pub fn record_execution_result_cache_bypass(&self, reason: &str) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        *metrics
            .execution_result_cache
            .bypass_reasons
            .entry(reason.to_string())
            .or_default() += 1;
    }

    /// # Panics
    ///
    /// Panics if the runtime metrics lock is poisoned.
    pub fn record_execution_result_cache_eviction(&self, evictions: u64) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.execution_result_cache.evictions += evictions;
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn plan_cache_entry_count(&self) -> usize {
        self.plan_cache.lock().expect("plan cache").entries.len()
    }
}
