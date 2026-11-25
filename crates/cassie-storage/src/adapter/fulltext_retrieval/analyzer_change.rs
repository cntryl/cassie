//! Changing a full-text index analyzer publishes a new analyzer generation.
//!
//! The collection write gate is held for the whole rebuild. Document writers
//! take the same gate and re-read index metadata after acquiring it, so a
//! concurrent writer either commits before the rebuild (and is re-analyzed by
//! it) or waits and then maintains the index with the new analyzer. The new
//! postings are staged in bounded transactions while the index is unpublished,
//! so readers take the exact fallback with the previous analyzer; the index
//! metadata and the new options then commit in one transaction.

use std::collections::BTreeMap;

use super::{CassieError, IndexMeta, Midge};
use cassie_search::analyzer::AnalyzerConfig;

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
    pub fn alter_fulltext_index_analyzer<E: From<CassieError>>(
        &self,
        collection: &str,
        name: &str,
        update: impl FnOnce(&mut BTreeMap<String, String>) -> Result<(), E>,
        publish: impl FnOnce(IndexMeta),
    ) -> Result<(), E> {
        let collection = self.canonical_collection_name(collection);
        let write_gate = self.collection_write_gate(&collection);
        let _write_guard = write_gate.lock();
        let tx = self.begin_data_readonly_tx_for(&collection)?;
        let key = Self::index_key(&collection, name);
        let raw = tx.get(&key).map_err(CassieError::from)?.ok_or_else(|| {
            CassieError::CatalogObjectNotFound {
                kind: crate::adapter::CatalogObjectKind::Index,
                name: name.to_string(),
            }
        })?;
        let mut metadata: IndexMeta = serde_json::from_slice(&raw)
            .map_err(|error| CassieError::Parse(format!("invalid index metadata: {error}")))?;
        if metadata.kind != cassie_core::catalog::declarations::IndexKind::FullText {
            return Err(CassieError::Unsupported(format!(
                "index '{name}' is not a full-text index"
            ))
            .into());
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
        cassie_sql::definition_guard::index(
            metadata.definition_semantics_version,
            &metadata.expressions,
            metadata.predicate.as_deref(),
        )
        .map_err(CassieError::from)?;
        drop(tx);
        let published =
            serde_json::to_vec(&metadata).map_err(|error| CassieError::Parse(error.to_string()))?;
        let tally = self.rebuild_fulltext_index_staged(
            &metadata,
            |tx| tx.put(key, published, None).map_err(CassieError::from),
            Some(super::super::schema_write_control::SchemaWritePausePoint::FulltextAnalyzerChange),
        )?;
        self.fulltext_maintenance_metrics.record(&tally);
        publish(metadata);
        Ok(())
    }
}
