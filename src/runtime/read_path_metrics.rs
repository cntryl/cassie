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

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_graph_traversal(
        &self,
        graph: &str,
        strategy: &str,
        max_depth: usize,
        rows: usize,
        stop_reason: &str,
    ) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.graph.traversals += 1;
        metrics.graph.rows = metrics.graph.rows.saturating_add(rows as u64);
        metrics.graph.max_depth = metrics.graph.max_depth.max(max_depth as u64);
        metrics.graph.last_graph = graph.to_string();
        metrics.graph.last_strategy = strategy.to_string();
        metrics.graph.last_stop_reason = stop_reason.to_string();
    }

    /// Record why graph traversal used the row-backed correctness path.
    pub(crate) fn record_graph_fallback(&self, reason: &str) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.graph.last_fallback_reason = reason.to_string();
    }

    pub(crate) fn record_graph_read_evidence(&self, reads: usize, candidates: usize) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.graph.reads = metrics.graph.reads.saturating_add(reads as u64);
        metrics.graph.candidates = metrics.graph.candidates.saturating_add(candidates as u64);
        metrics.graph.last_reads = reads as u64;
        metrics.graph.last_candidates = candidates as u64;
    }
}
