use super::*;

impl RuntimeState {
    pub fn record_read_path_collection_scan(&self, collection: &str, fields: usize) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.read_paths.collection_scans += 1;
        metrics.read_paths.collection_scan_fields += fields as u64;
        metrics.read_paths.last_collection_scan_collection = collection.to_string();
    }

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
}
