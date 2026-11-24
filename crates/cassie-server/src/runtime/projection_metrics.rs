use super::RuntimeState;

#[derive(Debug, Clone, Copy, Default)]
pub struct ProjectionWriteStats {
    pub row_puts: u64,
    pub row_deletes: u64,
    pub index_puts: u64,
    pub index_deletes: u64,
    pub metadata_puts: u64,
    pub metadata_deletes: u64,
    pub duplicate_checks: u64,
    pub batch_flushes: u64,
    pub rebuild_target_puts: u64,
    pub activation_metadata_writes: u64,
}

impl RuntimeState {
    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_projection_write_batch(
        &self,
        projection: impl Into<String>,
        stats: &ProjectionWriteStats,
    ) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.projections.last_projection = projection.into();
        metrics.projections.write_row_puts = metrics
            .projections
            .write_row_puts
            .saturating_add(stats.row_puts);
        metrics.projections.write_row_deletes = metrics
            .projections
            .write_row_deletes
            .saturating_add(stats.row_deletes);
        metrics.projections.write_index_puts = metrics
            .projections
            .write_index_puts
            .saturating_add(stats.index_puts);
        metrics.projections.write_index_deletes = metrics
            .projections
            .write_index_deletes
            .saturating_add(stats.index_deletes);
        metrics.projections.write_metadata_puts = metrics
            .projections
            .write_metadata_puts
            .saturating_add(stats.metadata_puts);
        metrics.projections.write_metadata_deletes = metrics
            .projections
            .write_metadata_deletes
            .saturating_add(stats.metadata_deletes);
        metrics.projections.write_duplicate_checks = metrics
            .projections
            .write_duplicate_checks
            .saturating_add(stats.duplicate_checks);
        metrics.projections.write_batch_flushes = metrics
            .projections
            .write_batch_flushes
            .saturating_add(stats.batch_flushes);
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_projection_index_writes(
        &self,
        projection: impl Into<String>,
        puts: u64,
        deletes: u64,
    ) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.projections.last_projection = projection.into();
        metrics.projections.write_index_puts =
            metrics.projections.write_index_puts.saturating_add(puts);
        metrics.projections.write_index_deletes = metrics
            .projections
            .write_index_deletes
            .saturating_add(deletes);
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_projection_metadata_writes(
        &self,
        projection: impl Into<String>,
        puts: u64,
        deletes: u64,
    ) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.projections.last_projection = projection.into();
        metrics.projections.write_metadata_puts =
            metrics.projections.write_metadata_puts.saturating_add(puts);
        metrics.projections.write_metadata_deletes = metrics
            .projections
            .write_metadata_deletes
            .saturating_add(deletes);
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_projection_rebuild_writes(
        &self,
        projection: impl Into<String>,
        target_puts: u64,
        flushes: u64,
    ) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.projections.last_projection = projection.into();
        metrics.projections.write_rebuild_target_puts = metrics
            .projections
            .write_rebuild_target_puts
            .saturating_add(target_puts);
        metrics.projections.write_batch_flushes = metrics
            .projections
            .write_batch_flushes
            .saturating_add(flushes);
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_projection_activation_write(&self, projection: impl Into<String>) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.projections.last_projection = projection.into();
        metrics.projections.write_activation_metadata_writes = metrics
            .projections
            .write_activation_metadata_writes
            .saturating_add(1);
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_projection_replay(
        &self,
        projection: impl Into<String>,
        applied: u64,
        skipped: u64,
    ) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.projections.replay_batches += 1;
        metrics.projections.replay_events_applied += applied;
        metrics.projections.replay_duplicates_skipped += skipped;
        metrics.projections.last_projection = projection.into();
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_projection_replay_error(
        &self,
        projection: impl Into<String>,
        error: impl Into<String>,
    ) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.projections.replay_errors += 1;
        metrics.projections.last_projection = projection.into();
        metrics.projections.last_error = error.into();
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_materialized_projection_build(&self, projection: impl Into<String>) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.projections.materialized_builds += 1;
        metrics.projections.last_projection = projection.into();
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_materialized_projection_refresh(&self, projection: impl Into<String>) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.projections.materialized_refreshes += 1;
        metrics.projections.last_projection = projection.into();
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_projection_swap(&self, projection: impl Into<String>) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.projections.version_swaps += 1;
        metrics.projections.last_projection = projection.into();
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_projection_stale_mark(&self, projection: impl Into<String>) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.projections.stale_marks += 1;
        metrics.projections.last_projection = projection.into();
        metrics.projections.last_state = "stale".to_string();
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_projection_hash_update(&self, projection: impl Into<String>) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.projections.row_hash_updates += 1;
        metrics.projections.range_hash_updates += 1;
        metrics.projections.root_hash_updates += 1;
        metrics.projections.last_projection = projection.into();
        metrics.projections.last_state = "current".to_string();
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_projection_rebuild_verification(
        &self,
        projection: impl Into<String>,
        failed: bool,
    ) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.projections.rebuild_verifications += 1;
        if failed {
            metrics.projections.rebuild_verification_failures += 1;
            metrics.projections.last_state = "failed".to_string();
        } else {
            metrics.projections.last_state = "verified".to_string();
        }
        metrics.projections.last_projection = projection.into();
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_projection_integrity_verification(
        &self,
        projection: impl Into<String>,
        failed: bool,
    ) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.projections.integrity_verifications += 1;
        if failed {
            metrics.projections.integrity_verification_failures += 1;
            metrics.projections.last_state = "failed".to_string();
        } else {
            metrics.projections.last_state = "verified".to_string();
        }
        metrics.projections.last_projection = projection.into();
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_projection_manifest_export(&self, projection: impl Into<String>) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.projections.consistency_exports += 1;
        metrics.projections.last_projection = projection.into();
        metrics.projections.last_state = "manifest_exported".to_string();
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_projection_consistency_check(
        &self,
        projection: impl Into<String>,
        state: impl Into<String>,
        mismatches: u64,
        stale_manifests: u64,
        incompatible_manifests: u64,
    ) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.projections.consistency_checks += 1;
        metrics.projections.consistency_mismatches = metrics
            .projections
            .consistency_mismatches
            .saturating_add(mismatches);
        metrics.projections.consistency_stale_manifests = metrics
            .projections
            .consistency_stale_manifests
            .saturating_add(stale_manifests);
        metrics.projections.consistency_incompatible_manifests = metrics
            .projections
            .consistency_incompatible_manifests
            .saturating_add(incompatible_manifests);
        metrics.projections.last_projection = projection.into();
        metrics.projections.last_state = state.into();
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_mixed_execution_optimized(&self, projection: impl Into<String>) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.projections.mixed_execution_optimized += 1;
        metrics.projections.last_projection = projection.into();
        metrics.projections.last_state = "optimized".to_string();
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_mixed_execution_fallback(
        &self,
        projection: impl Into<String>,
        reason: impl Into<String>,
    ) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.projections.mixed_execution_fallbacks += 1;
        metrics.projections.last_projection = projection.into();
        metrics.projections.last_state = "fallback".to_string();
        metrics.projections.last_fallback_reason = reason.into();
    }
}
