//! The committed embedding manifest (`tests/corpus/embeddings/manifest.json`).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};

pub const README_HINT: &str = "see tools/embeddings/README.md to generate the files";

#[derive(Debug, Deserialize)]
pub struct Manifest {
    pub schema_version: u32,
    pub redistribution: String,
    pub models: Vec<Model>,
    pub sets: Vec<DocumentSet>,
    pub queries: Queries,
    pub tenants: Tenants,
    pub ground_truth: GroundTruthPolicy,
    pub files: Vec<FileEntry>,
}

#[derive(Debug, Deserialize)]
pub struct Model {
    pub key: String,
    pub id: String,
    pub revision: String,
    pub dimensions: usize,
    pub file_stem: String,
}

#[derive(Debug, Deserialize)]
pub struct DocumentSet {
    pub name: String,
    pub documents: usize,
}

#[derive(Debug, Deserialize)]
pub struct QuerySplit {
    pub count: usize,
    pub ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct Queries {
    pub tuning: QuerySplit,
    pub held_out: QuerySplit,
}

#[derive(Debug, Deserialize)]
pub struct TenantColumn {
    pub column: String,
    pub selectivity_percent: u32,
}

#[derive(Debug, Deserialize)]
pub struct Tenants {
    pub columns: Vec<TenantColumn>,
}

#[derive(Debug, Deserialize)]
pub struct GroundTruthPolicy {
    pub k: usize,
    pub metrics: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub sha256: String,
    pub bytes: u64,
    pub rows: usize,
    pub dimensions: Option<usize>,
}

impl Manifest {
    /// Parses the manifest JSON text.
    ///
    /// # Errors
    ///
    /// Returns a message when the JSON does not match the manifest schema.
    pub fn parse(text: &str) -> Result<Self, String> {
        serde_json::from_str(text).map_err(|error| format!("manifest is not valid: {error}"))
    }

    /// Reads and parses the manifest at `path`.
    ///
    /// # Errors
    ///
    /// Returns a message when the file cannot be read or parsed.
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|error| format!("cannot read manifest {}: {error}", path.display()))?;
        Self::parse(&text)
    }

    #[must_use]
    pub fn model(&self, key: &str) -> Option<&Model> {
        self.models.iter().find(|model| model.key == key)
    }

    #[must_use]
    pub fn set(&self, name: &str) -> Option<&DocumentSet> {
        self.sets.iter().find(|set| set.name == name)
    }

    #[must_use]
    pub fn file(&self, name: &str) -> Option<&FileEntry> {
        self.files.iter().find(|file| file.name == name)
    }

    /// Names of the document vector, document id, query vector and query id files
    /// for one model and document set, in that order.
    #[must_use]
    pub fn file_names(model: &Model, set: &DocumentSet) -> [String; 4] {
        [
            format!("cqadupstack-{}-{}.f32", model.file_stem, set.name),
            format!("cqadupstack-{}.ids.txt", set.name),
            format!("cqadupstack-{}-queries.f32", model.file_stem),
            "cqadupstack-queries.ids.txt".to_string(),
        ]
    }

    /// Checks internal consistency: dimensions, counts, disjoint query splits,
    /// recorded file sizes and the tenant columns.
    ///
    /// # Errors
    ///
    /// Returns the first inconsistency found.
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 {
            return Err(format!(
                "unsupported schema_version {}",
                self.schema_version
            ));
        }
        if !self.redistribution.contains("No corpus text") {
            return Err("manifest must state that data is not redistributed".to_string());
        }
        self.validate_queries()?;
        self.validate_tenants()?;
        for file in &self.files {
            let width = file.dimensions.map_or(0, |dimensions| dimensions * 4);
            let expected = u64::try_from(file.rows * width).map_err(|error| error.to_string())?;
            if file.dimensions.is_some() && file.bytes != expected {
                return Err(format!(
                    "{} records {} bytes but rows x dimensions x 4 is {expected}",
                    file.name, file.bytes
                ));
            }
            if file.sha256.len() != 64 || !file.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(format!("{} has a malformed sha256", file.name));
            }
        }
        for model in &self.models {
            for set in &self.sets {
                self.validate_recorded_set(model, set)?;
            }
        }
        Ok(())
    }

    fn validate_queries(&self) -> Result<(), String> {
        let split = |name: &str, split: &QuerySplit| {
            // An empty id list means the split has not been frozen yet.
            if !split.ids.is_empty() && split.ids.len() != split.count {
                return Err(format!(
                    "{name} declares {} but lists {}",
                    split.count,
                    split.ids.len()
                ));
            }
            Ok(())
        };
        split("tuning", &self.queries.tuning)?;
        split("held_out", &self.queries.held_out)?;
        let tuning: BTreeSet<&String> = self.queries.tuning.ids.iter().collect();
        let held_out: BTreeSet<&String> = self.queries.held_out.ids.iter().collect();
        if tuning.len() != self.queries.tuning.ids.len()
            || held_out.len() != self.queries.held_out.ids.len()
        {
            return Err("a query id is repeated inside a split".to_string());
        }
        if let Some(shared) = tuning.intersection(&held_out).next() {
            return Err(format!("query {shared} is in both tuning and held-out"));
        }
        Ok(())
    }

    fn validate_tenants(&self) -> Result<(), String> {
        let selectivities: Vec<u32> = self
            .tenants
            .columns
            .iter()
            .map(|column| column.selectivity_percent)
            .collect();
        if selectivities != [1, 10, 50] {
            return Err(format!(
                "tenant selectivities must be 1/10/50, got {selectivities:?}"
            ));
        }
        for column in &self.tenants.columns {
            if column.column != format!("t{}", column.selectivity_percent) {
                return Err(format!(
                    "tenant column {} does not match its selectivity",
                    column.column
                ));
            }
        }
        Ok(())
    }

    fn validate_recorded_set(&self, model: &Model, set: &DocumentSet) -> Result<(), String> {
        let [docs, ids, queries, query_ids] = Self::file_names(model, set);
        let query_total = self.queries.tuning.count + self.queries.held_out.count;
        for (name, rows, dimensions) in [
            (docs, set.documents, Some(model.dimensions)),
            (ids, set.documents, None),
            (queries, query_total, Some(model.dimensions)),
            (query_ids, query_total, None),
        ] {
            let Some(file) = self.file(&name) else {
                continue; // not generated yet
            };
            if file.rows != rows || file.dimensions != dimensions {
                return Err(format!(
                    "{name} records rows={} dimensions={:?}, expected rows={rows} dimensions={dimensions:?}",
                    file.rows, file.dimensions
                ));
            }
        }
        Ok(())
    }
}

/// Bucket in `0..100` of a document id: the first 8 hex digits of
/// `sha256("tenant:" + id)` as a `u32`, modulo 100.
#[must_use]
pub fn tenant_bucket(document_id: &str) -> u32 {
    let digest = Sha256::digest(format!("tenant:{document_id}").as_bytes());
    u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]]) % 100
}

/// Whether a document belongs to the tenant column with `selectivity_percent`.
#[must_use]
pub fn in_tenant(document_id: &str, selectivity_percent: u32) -> bool {
    tenant_bucket(document_id) < selectivity_percent
}

/// Directory that holds generated files, relative to the repository root.
#[must_use]
pub fn embeddings_dir(root: &Path) -> PathBuf {
    root.join("target").join("embeddings")
}
