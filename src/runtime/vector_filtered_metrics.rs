//! Counters for the scalar-filtered vector candidate strategy (#1363).
//!
//! One record is published per filtered vector or hybrid statement. `paths` counts the selected
//! strategy per statement shape (`standalone.filtered-ann`, `standalone.bounded-exact`,
//! `hybrid.bounded-exact`), `fallback_reasons` counts every attempt that declined to the full
//! exact scan or from filtered ANN to the bounded exact subset, and the work counters report what
//! the selected strategy actually read.

use std::collections::BTreeMap;

use serde::Serialize;

use super::{increment_boundary_counter, RuntimeState};

#[derive(Debug, Clone, Serialize, Default)]
pub struct VectorFilteredSnapshot {
    pub statements_total: u64,
    pub paths: BTreeMap<String, u64>,
    pub fallback_reasons: BTreeMap<String, u64>,
    /// Sum of eligible-subset sizes returned by the scalar index probes.
    pub eligible_subset_total: u64,
    /// ANN search windows issued (one per adaptive expansion step).
    pub ann_windows_total: u64,
    /// Candidates returned by ANN windows before the eligibility test.
    pub ann_candidates_total: u64,
    /// Candidates that passed the eligibility test and were selected for reranking.
    pub eligible_candidates_total: u64,
    /// Source rows fetched for exact rerank or residual rechecks.
    pub row_fetches_total: u64,
    pub exact_reranks_total: u64,
    pub last_path: String,
    pub last_fallback_reason: String,
}

/// What one filtered statement did.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct FilteredVectorWork {
    pub(crate) eligible_subset: usize,
    pub(crate) ann_windows: usize,
    pub(crate) ann_candidates: usize,
    pub(crate) eligible_candidates: usize,
    pub(crate) row_fetches: usize,
    pub(crate) exact_reranks: usize,
}

impl RuntimeState {
    /// Records the strategy a filtered statement completed with.
    ///
    /// # Panics
    ///
    /// Panics if the runtime metrics lock is poisoned.
    pub(crate) fn record_vector_filtered_path(&self, path: &str, work: &FilteredVectorWork) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        let filtered = &mut metrics.vector_filtered;
        filtered.statements_total += 1;
        increment_boundary_counter(&mut filtered.paths, path);
        filtered.eligible_subset_total += work.eligible_subset as u64;
        filtered.ann_windows_total += work.ann_windows as u64;
        filtered.ann_candidates_total += work.ann_candidates as u64;
        filtered.eligible_candidates_total += work.eligible_candidates as u64;
        filtered.row_fetches_total += work.row_fetches as u64;
        filtered.exact_reranks_total += work.exact_reranks as u64;
        filtered.last_path = path.to_string();
    }

    /// Records an attempt that declined or degraded to a more exact strategy.
    ///
    /// # Panics
    ///
    /// Panics if the runtime metrics lock is poisoned.
    pub(crate) fn record_vector_filtered_fallback(&self, reason: &str) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        increment_boundary_counter(&mut metrics.vector_filtered.fallback_reasons, reason);
        metrics.vector_filtered.last_fallback_reason = reason.to_string();
    }
}
