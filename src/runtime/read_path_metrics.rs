use super::{RuntimeMetricsState, RuntimeState};

impl RuntimeState {
    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_read_path_collection_scan(&self, collection: &str, fields: usize, rows: usize) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.read_paths.collection_scans += 1;
        metrics.read_paths.collection_scan_fields += fields as u64;
        metrics.read_paths.collection_scan_rows += rows as u64;
        metrics.read_paths.last_collection_scan_collection = collection.to_string();
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_read_path_point_lookup(&self, collection: &str, hit: bool) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.read_paths.point_lookup_scans += 1;
        if hit {
            metrics.read_paths.point_lookup_hits += 1;
        } else {
            metrics.read_paths.point_lookup_misses += 1;
        }
        metrics.read_paths.last_point_lookup_collection = collection.to_string();
        metrics.read_paths.last_point_lookup_hit = hit;
    }

    pub fn record_read_path_index_seek(&self, collection: &str, rows: usize, index: &str) {
        self.record_index_read_path(collection, rows, index, "index_seek", |metrics| {
            metrics.read_paths.index_seek_scans += 1;
        });
    }

    pub fn record_read_path_prefix_scan(&self, collection: &str, rows: usize, index: &str) {
        self.record_index_read_path(collection, rows, index, "prefix_scan", |metrics| {
            metrics.read_paths.prefix_scans += 1;
        });
    }

    pub fn record_read_path_range_scan(&self, collection: &str, rows: usize, index: &str) {
        self.record_index_read_path(collection, rows, index, "range_scan", |metrics| {
            metrics.read_paths.range_scans += 1;
        });
    }

    pub fn record_read_path_ordered_bounded_scan(
        &self,
        collection: &str,
        rows: usize,
        index: &str,
    ) {
        self.record_index_read_path(collection, rows, index, "ordered_bounded_scan", |metrics| {
            metrics.read_paths.ordered_bounded_scans += 1;
            metrics.read_paths.ordered_scans += 1;
            metrics.read_paths.ordered_rows += rows as u64;
            metrics.read_paths.last_ordered_scan_collection = collection.to_string();
            metrics.read_paths.last_ordered_scan_mode = "ordered_bounded_scan".to_string();
        });
    }

    pub fn record_read_path_storage_top_k(&self, collection: &str, rows: usize) {
        self.record_ordered_read_path(collection, rows, "storage_top_k", |metrics| {
            metrics.read_paths.storage_top_k_scans += 1;
        });
    }

    pub fn record_read_path_keyset(&self, collection: &str, rows: usize) {
        self.record_ordered_read_path(collection, rows, "keyset", |metrics| {
            metrics.read_paths.keyset_scans += 1;
        });
    }

    pub fn record_read_path_degraded_offset(&self, collection: &str, rows: usize) {
        self.record_ordered_read_path(collection, rows, "degraded_offset", |metrics| {
            metrics.read_paths.degraded_offset_scans += 1;
        });
    }

    pub fn record_read_path_heap_top_k(&self, collection: &str, rows: usize) {
        self.record_ordered_read_path(collection, rows, "heap_top_k", |metrics| {
            metrics.read_paths.heap_top_k_scans += 1;
        });
    }

    fn record_ordered_read_path<F>(&self, collection: &str, rows: usize, mode: &str, update: F)
    where
        F: FnOnce(&mut RuntimeMetricsState),
    {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.read_paths.ordered_scans += 1;
        metrics.read_paths.ordered_rows += rows as u64;
        metrics.read_paths.last_ordered_scan_collection = collection.to_string();
        metrics.read_paths.last_ordered_scan_mode = mode.to_string();
        update(&mut metrics);
    }

    fn record_index_read_path<F>(
        &self,
        collection: &str,
        rows: usize,
        index: &str,
        mode: &str,
        update: F,
    ) where
        F: FnOnce(&mut RuntimeMetricsState),
    {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.read_paths.last_index_scan_collection = collection.to_string();
        metrics.read_paths.last_index_scan_index = index.to_string();
        metrics.read_paths.last_index_scan_mode = mode.to_string();
        let _ = rows;
        update(&mut metrics);
    }
}

/// What one graph read counted; recorded once when its traversal ends.
#[derive(Debug, Clone, Copy, Default)]
pub struct GraphReadCounts {
    pub index_entries: u64,
    pub row_fetches: u64,
    pub full_rows: u64,
    pub overlay_changes: u64,
    pub cache_hits: u64,
    pub validation_rows: u64,
    pub frontier_peak: u64,
}

impl RuntimeState {
    /// Forces every graph read onto the exact full-read path. Used by equivalence tests and
    /// as an operator escape hatch.
    pub fn set_graph_exact_reads(&self, exact: bool) {
        self.graph_exact_reads
            .store(exact, std::sync::atomic::Ordering::SeqCst);
    }

    #[must_use]
    pub fn graph_exact_reads(&self) -> bool {
        self.graph_exact_reads
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_graph_reads(&self, counts: GraphReadCounts) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        let paths = &mut metrics.read_paths;
        paths.graph_index_entries += counts.index_entries;
        paths.graph_row_fetches += counts.row_fetches;
        paths.graph_full_rows += counts.full_rows;
        paths.graph_overlay_changes += counts.overlay_changes;
        paths.graph_cache_hits += counts.cache_hits;
        paths.graph_validation_rows += counts.validation_rows;
        paths.graph_last_frontier_peak = counts.frontier_peak;
    }

    /// Records, for the latest joined `GRAPH_TABLE`, each pattern's edge hops and
    /// selected paths in pattern order.
    ///
    /// # Panics
    ///
    /// Panics if the runtime metrics lock is poisoned.
    pub fn record_graph_branches(&self, expansions: Vec<u64>, paths: Vec<u64>) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.read_paths.graph_last_branch_expansions = expansions;
        metrics.read_paths.graph_last_branch_paths = paths;
    }
}
