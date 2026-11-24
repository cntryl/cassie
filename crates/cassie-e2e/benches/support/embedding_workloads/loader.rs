//! Loads and verifies the generated vector files and caches exact ground truth.

use std::path::{Path, PathBuf};

use crate::embedding_workloads::ground_truth::{
    decode_truths, encode_truths, exact_top_k, Metric, Truth,
};
use crate::embedding_workloads::manifest::{in_tenant, Manifest, README_HINT};
use crate::embedding_workloads::synthetic;
use crate::embedding_workloads::vector_file::{
    read_f32_rows, read_lines, sha256_file, verify_checksum,
};

/// Query splits in the order they appear in the query files.
pub const SPLITS: [&str; 2] = ["tuning", "held_out"];

pub struct Workload {
    pub model_key: String,
    pub model_stem: String,
    pub model_id: String,
    pub model_revision: String,
    pub set: String,
    pub dimensions: usize,
    pub documents: Vec<f32>,
    pub document_ids: Vec<String>,
    pub queries: Vec<f32>,
    pub query_ids: Vec<String>,
    pub tuning_count: usize,
    pub documents_sha256: String,
    pub queries_sha256: String,
    pub dir: PathBuf,
}

impl Workload {
    /// Verifies the four files of `model_key` and `set` against the manifest checksums
    /// and loads them.
    ///
    /// # Errors
    ///
    /// Returns a message that points at `tools/embeddings/README.md` when a file is
    /// missing, unrecorded in the manifest, or fails its checksum.
    pub fn load(
        manifest: &Manifest,
        dir: &Path,
        model_key: &str,
        set: &str,
    ) -> Result<Self, String> {
        let model = manifest
            .model(model_key)
            .ok_or_else(|| format!("manifest has no model {model_key}"))?;
        let set_entry = manifest
            .set(set)
            .ok_or_else(|| format!("manifest has no set {set}"))?;
        let names = Manifest::file_names(model, set_entry);
        for name in &names {
            let entry = manifest.file(name).ok_or_else(|| {
                format!("{name} has no recorded sha256 in the manifest; {README_HINT}")
            })?;
            verify_checksum(&dir.join(name), &entry.sha256, README_HINT)?;
        }
        let documents = read_f32_rows(&dir.join(&names[0]), model.dimensions)?;
        let document_ids = read_lines(&dir.join(&names[1]))?;
        let queries = read_f32_rows(&dir.join(&names[2]), model.dimensions)?;
        let query_ids = read_lines(&dir.join(&names[3]))?;
        if documents.len() != set_entry.documents * model.dimensions
            || document_ids.len() != set_entry.documents
        {
            return Err(format!(
                "{} does not hold {} documents",
                names[0], set_entry.documents
            ));
        }
        if query_ids
            != [
                &manifest.queries.tuning.ids[..],
                &manifest.queries.held_out.ids[..],
            ]
            .concat()
        {
            return Err(format!(
                "{} does not list the manifest query splits in order",
                names[3]
            ));
        }
        Ok(Self {
            model_key: model_key.to_string(),
            model_stem: model.file_stem.clone(),
            model_id: model.id.clone(),
            model_revision: model.revision.clone(),
            set: set.to_string(),
            dimensions: model.dimensions,
            documents,
            document_ids,
            queries,
            query_ids,
            tuning_count: manifest.queries.tuning.count,
            documents_sha256: manifest
                .file(&names[0])
                .map_or_else(String::new, |f| f.sha256.clone()),
            queries_sha256: manifest
                .file(&names[2])
                .map_or_else(String::new, |f| f.sha256.clone()),
            dir: dir.to_path_buf(),
        })
    }

    /// Generates the deterministic synthetic control for `set` and verifies its checksums
    /// against the manifest.
    ///
    /// # Errors
    ///
    /// Returns a message when the manifest has no such set, the query splits are not
    /// frozen, or a generated checksum differs from the recorded one.
    pub fn synthetic(manifest: &Manifest, dir: &Path, set: &str) -> Result<Self, String> {
        let control = &manifest.synthetic_control;
        let set_entry = manifest
            .set(set)
            .ok_or_else(|| format!("manifest has no set {set}"))?;
        let query_ids = [
            &manifest.queries.tuning.ids[..],
            &manifest.queries.held_out.ids[..],
        ]
        .concat();
        if query_ids.is_empty() {
            return Err(format!("the query splits are not frozen; {README_HINT}"));
        }
        let vectors = synthetic::generate(control, set_entry.documents, query_ids.len());
        let documents_sha256 = synthetic::checksum(&vectors.documents);
        let queries_sha256 = synthetic::checksum(&vectors.queries);
        let mismatches: Vec<String> = [(set, &documents_sha256), ("queries", &queries_sha256)]
            .into_iter()
            .filter(|(name, actual)| control.sha256.get(*name) != Some(*actual))
            .map(|(name, actual)| format!("{name} hashes to {actual}"))
            .collect();
        if !mismatches.is_empty() {
            return Err(format!(
                "synthetic control differs from the manifest sha256 record: {}",
                mismatches.join("; ")
            ));
        }
        Ok(Self {
            model_key: control.name.clone(),
            model_stem: control.name.clone(),
            model_id: "deterministic-synthetic-control".to_string(),
            model_revision: "manifest synthetic_control".to_string(),
            set: set.to_string(),
            dimensions: control.dimensions,
            documents: vectors.documents,
            document_ids: (0..set_entry.documents)
                .map(|ordinal| format!("syn-{ordinal:06}"))
                .collect(),
            queries: vectors.queries,
            query_ids,
            tuning_count: manifest.queries.tuning.count,
            documents_sha256,
            queries_sha256,
            dir: dir.to_path_buf(),
        })
    }

    #[must_use]
    pub fn document(&self, ordinal: usize) -> &[f32] {
        &self.documents[ordinal * self.dimensions..(ordinal + 1) * self.dimensions]
    }

    #[must_use]
    pub fn query(&self, index: usize) -> &[f32] {
        &self.queries[index * self.dimensions..(index + 1) * self.dimensions]
    }

    /// Row ordinals of the documents in the tenant column with `selectivity_percent`.
    #[must_use]
    pub fn tenant_subset(&self, selectivity_percent: u32) -> Vec<usize> {
        (0..self.document_ids.len())
            .filter(|&ordinal| in_tenant(&self.document_ids[ordinal], selectivity_percent))
            .collect()
    }

    /// Query indexes of a split, capped at `limit` when given.
    #[must_use]
    pub fn split_queries(&self, split: &str, limit: Option<usize>) -> Vec<usize> {
        let range = if split == "tuning" {
            0..self.tuning_count
        } else {
            self.tuning_count..self.query_ids.len()
        };
        range.take(limit.unwrap_or(usize::MAX)).collect()
    }

    /// Exact top-`k` truth for every query index in `queries`, read from the checksummed
    /// cache next to the vectors when it is current, else computed by brute force on all
    /// cores and cached.
    ///
    /// # Errors
    ///
    /// Returns a message when the cache cannot be written or read.
    pub fn truths(
        &self,
        metric: Metric,
        filter: &str,
        subset: Option<&[usize]>,
        queries: &[usize],
        k: usize,
    ) -> Result<Vec<Truth>, String> {
        let stem = format!(
            "cqadupstack-{}-{}-{}-{}-top{k}-q{}",
            self.model_stem,
            self.set,
            metric.name(),
            filter,
            queries.len()
        );
        let data = self.dir.join(format!("{stem}.truth"));
        let sidecar = self.dir.join(format!("{stem}.truth.sha256"));
        let header = format!(
            "# docs={} queries={} k={k}\n",
            self.documents_sha256, self.queries_sha256
        );
        if let (Ok(text), Ok(recorded)) = (
            std::fs::read_to_string(&data),
            std::fs::read_to_string(&sidecar),
        ) {
            let current = sha256_file(&data)?;
            if text.starts_with(&header) && recorded.trim() == current {
                if let Ok(truths) = decode_truths(&text[header.len()..]) {
                    if truths.len() == queries.len() {
                        return Ok(truths);
                    }
                }
            }
        }
        let truths = self.compute_truths(metric, subset, queries, k);
        std::fs::write(&data, format!("{header}{}", encode_truths(&truths)))
            .map_err(|error| format!("cannot write {}: {error}", data.display()))?;
        std::fs::write(&sidecar, format!("{}\n", sha256_file(&data)?))
            .map_err(|error| format!("cannot write {}: {error}", sidecar.display()))?;
        Ok(truths)
    }

    fn compute_truths(
        &self,
        metric: Metric,
        subset: Option<&[usize]>,
        queries: &[usize],
        k: usize,
    ) -> Vec<Truth> {
        let workers = std::thread::available_parallelism().map_or(1, usize::from);
        let chunk = queries.len().div_ceil(workers).max(1);
        std::thread::scope(|scope| {
            let handles: Vec<_> = queries
                .chunks(chunk)
                .map(|part| {
                    scope.spawn(move || {
                        part.iter()
                            .map(|&query| {
                                exact_top_k(
                                    metric,
                                    &self.documents,
                                    self.dimensions,
                                    self.query(query),
                                    subset,
                                    k,
                                )
                            })
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            handles
                .into_iter()
                .flat_map(|handle| handle.join().expect("truth worker"))
                .collect()
        })
    }
}
