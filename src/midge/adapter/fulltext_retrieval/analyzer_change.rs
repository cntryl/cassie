//! Changing a full-text index analyzer publishes a new analyzer generation.
//!
//! The collection write gate is held for the whole rebuild. Document writers
//! take the same gate and re-read index metadata after acquiring it, so a
//! concurrent writer either commits before the rebuild (and is re-analyzed by
//! it) or waits and then maintains the index with the new analyzer. The new
//! postings, manifest and index options commit in one transaction, so readers
//! keep the previous generation and analyzer until it commits.

use std::collections::BTreeMap;

use super::{CassieError, IndexMeta, Midge};
use crate::search::analyzer::AnalyzerConfig;

/// Option recording the published analyzer generation; absent means generation 1.
pub(crate) const ANALYZER_VERSION_OPTION: &str = "analyzer_version";

impl Midge {
    /// Applies `update` to a full-text index's options, rebuilds its postings with
    /// the resulting analyzer and publishes both atomically.
    ///
    /// # Errors
    ///
    /// Returns an error when the index is missing or not full-text, the new
    /// options do not form a valid analyzer, or storage fails. Nothing is
    /// published on error.
    pub(crate) fn alter_fulltext_index_analyzer(
        &self,
        collection: &str,
        name: &str,
        update: impl FnOnce(&mut BTreeMap<String, String>) -> Result<(), CassieError>,
        publish: impl FnOnce(IndexMeta),
    ) -> Result<(), CassieError> {
        let collection = self.canonical_collection_name(collection);
        let write_gate = self.collection_write_gate(&collection);
        let _write_guard = write_gate.lock();
        let mut tx = self.begin_data_rw_tx_for(&collection)?;
        let key = Self::index_key(&collection, name);
        let raw = tx.get(&key).map_err(CassieError::from)?.ok_or_else(|| {
            CassieError::CatalogObjectNotFound {
                kind: crate::app::CatalogObjectKind::Index,
                name: name.to_string(),
            }
        })?;
        let mut metadata: IndexMeta = serde_json::from_slice(&raw)
            .map_err(|error| CassieError::Parse(format!("invalid index metadata: {error}")))?;
        if metadata.kind != crate::catalog::IndexKind::FullText {
            return Err(CassieError::Unsupported(format!(
                "index '{name}' is not a full-text index"
            )));
        }
        let mut options = metadata.options.clone();
        update(&mut options)?;
        AnalyzerConfig::from_index_options(&options).map_err(CassieError::Unsupported)?;
        let version = options
            .get(ANALYZER_VERSION_OPTION)
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(1);
        options.insert(
            ANALYZER_VERSION_OPTION.to_string(),
            version.saturating_add(1).to_string(),
        );
        metadata.options = options;
        crate::sql::definition_guard::index(&metadata)?;
        tx.put(
            key,
            serde_json::to_vec(&metadata).map_err(|error| CassieError::Parse(error.to_string()))?,
            None,
        )
        .map_err(CassieError::from)?;
        let generation = self.collection_generation(&collection)?;
        let tally =
            self.rebuild_fulltext_index_in_tx(&mut tx, &collection, &metadata, generation)?;
        super::super::schema_write_control::pause_before_schema_write_commit(
            super::super::schema_write_control::SchemaWritePausePoint::FulltextAnalyzerChange,
        );
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)?;
        self.fulltext_maintenance_metrics.record(&tally);
        publish(metadata);
        Ok(())
    }
}
