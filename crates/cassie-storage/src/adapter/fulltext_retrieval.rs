use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{
    collect_scan, decode_row_with_hooks, key_encoding, CassieError, IndexMeta, Midge, Query,
    RowSchema,
};
use cassie_search::analyzer::AnalyzerConfig;

#[path = "fulltext_retrieval/analyzer_change.rs"]
mod analyzer_change;
#[path = "fulltext_retrieval/blocks.rs"]
mod blocks;
#[path = "fulltext_retrieval/codec.rs"]
mod codec;
#[path = "fulltext_retrieval/controlled.rs"]
mod controlled;
#[path = "fulltext_retrieval/incremental.rs"]
mod incremental;
#[path = "fulltext_retrieval/layout.rs"]
mod layout;
#[path = "fulltext_retrieval/metrics.rs"]
mod metrics;
#[path = "fulltext_retrieval/staged_rebuild.rs"]
mod staged_rebuild;
#[path = "fulltext_retrieval/vocabulary.rs"]
mod vocabulary;

pub use controlled::{ControlledFulltextCandidateSet, ControlledRetrievalDocument};
pub(crate) use incremental::FulltextCommitChanges;
pub(crate) use metrics::{FulltextMaintenanceMetrics, FulltextWriteTally};

const STATE_VERSION: u32 = 4;
const FULLTEXT_INDEX_BUILD_WRITE_BATCH_SIZE: usize = 5_000;
/// Key plus value bytes one index-build transaction may carry. Posting blocks
/// reach 64 KiB each, so a batch bounded only by record count can exceed the
/// storage engine's transaction validation budget.
const FULLTEXT_INDEX_BUILD_WRITE_BATCH_BYTES: usize = 1024 * 1024;

/// Splits `record_bytes` (the size of each record, in write order) into
/// consecutive ranges bounded by record count and by total bytes. A record
/// larger than the byte bound gets a range of its own.
fn fulltext_build_ranges(record_bytes: &[usize]) -> Vec<std::ops::Range<usize>> {
    let mut ranges = Vec::new();
    let mut start = 0_usize;
    let mut bytes = 0_usize;
    for (index, record) in record_bytes.iter().enumerate() {
        let full = index - start == FULLTEXT_INDEX_BUILD_WRITE_BATCH_SIZE
            || bytes.saturating_add(*record) > FULLTEXT_INDEX_BUILD_WRITE_BATCH_BYTES;
        if full && index > start {
            ranges.push(start..index);
            start = index;
            bytes = 0;
        }
        bytes = bytes.saturating_add(*record);
    }
    if start < record_bytes.len() {
        ranges.push(start..record_bytes.len());
    }
    ranges
}

impl Midge {
    #[doc(hidden)]
    pub fn fulltext_artifact_prefix_for_diagnostics(
        &self,
        collection: &str,
        index_name: &str,
    ) -> Result<Vec<u8>, CassieError> {
        let index = self
            .get_index(collection, index_name)?
            .ok_or_else(|| CassieError::Parse(format!("missing full-text index: {index_name}")))?;
        let (relation_id, index_id) = Self::fulltext_storage_ids(&index)?;
        Ok(Self::fulltext_index_artifact_prefix(relation_id, index_id))
    }

    pub(crate) fn fulltext_index_key(relation_id: u64, index_id: u64) -> Vec<u8> {
        key_encoding::fulltext_index_key(relation_id, index_id)
    }

    pub(crate) fn fulltext_index_collection_prefix(relation_id: u64) -> Vec<u8> {
        key_encoding::fulltext_index_collection_prefix(relation_id)
    }

    pub(crate) fn fulltext_index_artifact_prefix(relation_id: u64, index_id: u64) -> Vec<u8> {
        key_encoding::fulltext_index_artifact_prefix(relation_id, index_id)
    }

    pub(crate) fn fulltext_term_record_key(relation_id: u64, index_id: u64, term: &str) -> Vec<u8> {
        key_encoding::fulltext_term_record_key(relation_id, index_id, term)
    }

    pub(crate) fn fulltext_postings_prefix(relation_id: u64, index_id: u64) -> Vec<u8> {
        key_encoding::fulltext_postings_prefix(relation_id, index_id)
    }

    pub(crate) fn fulltext_term_postings_prefix(
        relation_id: u64,
        index_id: u64,
        term: &str,
    ) -> Vec<u8> {
        key_encoding::fulltext_term_postings_prefix(relation_id, index_id, term)
    }

    pub(crate) fn fulltext_term_postings_block_key(
        relation_id: u64,
        index_id: u64,
        term: &str,
        first_document_id: &str,
    ) -> Vec<u8> {
        key_encoding::fulltext_term_postings_block_key(
            relation_id,
            index_id,
            term,
            first_document_id,
        )
    }

    pub(crate) fn fulltext_document_stats_prefix(relation_id: u64, index_id: u64) -> Vec<u8> {
        key_encoding::fulltext_document_stats_prefix(relation_id, index_id)
    }

    pub(crate) fn fulltext_document_stats_key(
        relation_id: u64,
        index_id: u64,
        id: &str,
    ) -> Vec<u8> {
        key_encoding::fulltext_document_stats_key(relation_id, index_id, id)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistedFulltextPosting {
    pub document_id: String,
    pub term_frequency: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistedFulltextDocumentStats {
    pub doc_length: usize,
    /// Term frequency per term; the length of the term's position list.
    pub term_counts: BTreeMap<String, usize>,
    /// Sorted token positions per term. Positions count every token the
    /// tokenizer produced, so removed stop words keep their gaps.
    pub term_positions: BTreeMap<String, Vec<u32>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PersistedFulltextCandidateSet {
    pub total_documents: usize,
    pub average_document_length: f64,
    pub analyzer: AnalyzerConfig,
    pub document_frequency: BTreeMap<String, usize>,
    pub document_stats: BTreeMap<String, PersistedFulltextDocumentStats>,
    pub posting_block_reads: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PersistedFulltextIndexState {
    pub built_generation: u64,
    pub total_documents: usize,
    pub documents_with_text: usize,
    pub average_document_length: f64,
    /// Exact total token length of every document with text.
    pub total_document_length: usize,
    pub analyzer: AnalyzerConfig,
    pub document_stats: BTreeMap<String, PersistedFulltextDocumentStats>,
    pub postings: BTreeMap<String, Vec<PersistedFulltextPosting>>,
}

#[derive(Debug, Clone, PartialEq)]
struct FulltextIndexMetadata {
    version: u32,
    built_generation: u64,
    total_documents: usize,
    documents_with_text: usize,
    /// Exact token total, so BM25 averages are recomputed instead of drifting.
    total_document_length: usize,
    posting_terms: usize,
    analyzer: AnalyzerConfig,
}

/// Integrity counts of one term's posting blocks.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FulltextTermRecord {
    block_count: usize,
    posting_count: usize,
}

struct LoadedFulltextPostings {
    postings: BTreeMap<String, Vec<PersistedFulltextPosting>>,
    block_counts: BTreeMap<String, usize>,
    posting_counts: BTreeMap<String, usize>,
}

struct LoadedCandidatePostings {
    ids: std::collections::BTreeSet<String>,
    source_ids: std::collections::BTreeSet<String>,
    document_frequency: BTreeMap<String, usize>,
    block_reads: usize,
}

fn load_complete_fulltext_postings(
    tx: &crate::adapter::AdapterTransaction,
    relation_id: u64,
    index_id: u64,
) -> Result<LoadedFulltextPostings, CassieError> {
    let postings_prefix = Midge::fulltext_postings_prefix(relation_id, index_id);
    let posting_entries = collect_scan(
        tx.scan(&Query::new().prefix(postings_prefix.clone().into()))
            .map_err(CassieError::from)?,
    )?;
    let mut observed = Vec::new();
    for (key, raw) in posting_entries {
        let term = key_encoding::utf8_first_component_after_prefix(&key, &postings_prefix)
            .ok_or_else(|| CassieError::Parse("invalid fulltext posting key".to_string()))?;
        let block = codec::decode_postings(&raw).map_err(|error| {
            CassieError::Parse(format!("invalid fulltext posting for '{term}': {error}"))
        })?;
        observed.push((term, key, block));
    }
    let mut block_counts = BTreeMap::new();
    let mut posting_counts = BTreeMap::new();
    {
        let mut validators = BTreeMap::<&str, layout::TermBlockValidator<'_>>::new();
        for (term, key, block) in &observed {
            validators
                .entry(term.as_str())
                .or_insert_with(|| layout::TermBlockValidator::new(term, relation_id, index_id))
                .accept(key, block)?;
        }
        for (term, validator) in validators {
            block_counts.insert(term.to_string(), validator.block_count());
            posting_counts.insert(term.to_string(), validator.posting_count());
        }
    }
    let mut postings = BTreeMap::<String, Vec<PersistedFulltextPosting>>::new();
    for (term, _, block) in observed {
        postings.entry(term).or_default().extend(block);
    }
    Ok(LoadedFulltextPostings {
        postings,
        block_counts,
        posting_counts,
    })
}

fn validate_complete_artifact_counts(
    metadata: &FulltextIndexMetadata,
    term_records: &BTreeMap<String, FulltextTermRecord>,
    loaded: &LoadedFulltextPostings,
    document_stats: &BTreeMap<String, PersistedFulltextDocumentStats>,
) -> Result<(), CassieError> {
    let blocks_match = term_records.len() == loaded.block_counts.len()
        && term_records.iter().all(|(term, record)| {
            loaded.block_counts.get(term) == Some(&record.block_count)
                && loaded.posting_counts.get(term) == Some(&record.posting_count)
        });
    let total_length = document_stats
        .values()
        .map(layout::document_length)
        .sum::<usize>();
    if !blocks_match
        || metadata.posting_terms != term_records.len()
        || metadata.documents_with_text != document_stats.len()
        || metadata.total_document_length != total_length
    {
        return Err(CassieError::Parse(
            "fulltext persisted artifact counts are inconsistent".to_string(),
        ));
    }
    Ok(())
}

fn load_candidate_postings(
    tx: &crate::adapter::AdapterTransaction,
    relation_id: u64,
    index_id: u64,
    terms: &[String],
    allowed_ids: Option<&std::collections::BTreeSet<String>>,
) -> Result<LoadedCandidatePostings, CassieError> {
    let mut loaded = LoadedCandidatePostings {
        ids: std::collections::BTreeSet::new(),
        source_ids: std::collections::BTreeSet::new(),
        document_frequency: BTreeMap::new(),
        block_reads: 0,
    };
    for term in terms {
        let record = layout::read_term_record(tx, relation_id, index_id, term)?;
        let prefix = Midge::fulltext_term_postings_prefix(relation_id, index_id, term);
        let entries = collect_scan(
            tx.scan(&Query::new().prefix(prefix.into()))
                .map_err(CassieError::from)?,
        )?;
        loaded.block_reads = loaded.block_reads.saturating_add(entries.len());
        let mut validator = layout::TermBlockValidator::new(term, relation_id, index_id);
        let mut term_documents = std::collections::BTreeSet::new();
        for (key, raw) in entries {
            let postings = codec::decode_postings(&raw)?;
            validator.accept(&key, &postings)?;
            for posting in postings {
                term_documents.insert(posting.document_id.clone());
                if allowed_ids.is_none_or(|allowed| allowed.contains(&posting.document_id)) {
                    loaded.ids.insert(posting.document_id);
                }
            }
        }
        validator.finish(record.as_ref())?;
        loaded.source_ids.extend(term_documents.iter().cloned());
        loaded
            .document_frequency
            .insert(term.clone(), term_documents.len());
    }
    Ok(loaded)
}

impl Midge {
    /// # Errors
    ///
    /// Returns a storage error when any incomplete index cannot be inspected or rebuilt.
    pub fn reconcile_fulltext_indexes(&self) -> Result<(), CassieError> {
        let indexes = self
            .list_indexes()?
            .into_iter()
            .filter(|index| index.kind == cassie_core::catalog::declarations::IndexKind::FullText)
            .collect::<Vec<_>>();
        for index in indexes {
            let collection = self.canonical_collection_name(&index.collection);
            let write_gate = self.collection_write_gate(&collection);
            let _write_guard = write_gate.lock();
            let generation = self.collection_generation(&collection)?;
            let current = self.get_persisted_fulltext_index_state(&collection, &index.name);
            let valid = matches!(
                current,
                Ok(Some(ref state)) if state.built_generation == generation
            );
            if !valid {
                self.rebuild_fulltext_index_for_index(&index)?;
            }
        }
        Ok(())
    }

    /// Rebuilds `index` in bounded transactions under the collection write
    /// gate. Readers take the labelled exact fallback until it is republished.
    pub(crate) fn rebuild_fulltext_index_for_index(
        &self,
        index: &IndexMeta,
    ) -> Result<(), CassieError> {
        let collection = self.canonical_collection_name(&index.collection);
        let write_gate = self.collection_write_gate(&collection);
        let _write_guard = write_gate.lock();
        let tally = self.rebuild_fulltext_index_staged(
            index,
            |_| Ok(()),
            Some(super::schema_write_control::SchemaWritePausePoint::FulltextRebuildCommit),
        )?;
        self.fulltext_maintenance_metrics.record(&tally);
        Ok(())
    }

    /// Rebuilds a full-text index from its committed rows, discarding all
    /// incrementally maintained artifacts. Equivalence tests compare the state
    /// before and after.
    ///
    /// # Errors
    ///
    /// Returns an error when the index is missing or storage fails.
    #[doc(hidden)]
    pub fn rebuild_fulltext_index_for_diagnostics(
        &self,
        collection: &str,
        index_name: &str,
    ) -> Result<(), CassieError> {
        let collection = self.canonical_collection_name(collection);
        let index = self
            .get_index(&collection, index_name)?
            .ok_or_else(|| CassieError::Execution("fulltext fallback:missing-index".to_string()))?;
        let write_gate = self.collection_write_gate(&collection);
        let _write_guard = write_gate.lock();
        self.rebuild_fulltext_index_for_index(&index)
    }

    pub(super) fn rebuild_prepared_fulltext_index_for_index(
        &self,
        index: &IndexMeta,
    ) -> Result<(), CassieError> {
        if self.get_index(&index.collection, &index.name)?.is_some() {
            return self.rebuild_fulltext_index_for_index(index);
        }
        self.rebuild_fulltext_index_staged(index, |_| Ok(()), None)
            .map(|_| ())
    }

    /// # Errors
    ///
    /// Returns an error when persisted full-text metadata or artifacts are corrupt or unreadable.
    pub fn get_persisted_fulltext_index_state(
        &self,
        collection: &str,
        index_name: &str,
    ) -> Result<Option<PersistedFulltextIndexState>, CassieError> {
        let collection = self.canonical_collection_name(collection);
        let index = self
            .get_index(&collection, index_name)?
            .ok_or_else(|| CassieError::Execution("fulltext fallback:missing-index".to_string()))?;
        let (relation_id, index_id) = Self::fulltext_storage_ids(&index)?;
        let tx = self.begin_data_readonly_tx_for(&collection)?;
        let Some(raw) = tx
            .get(&Self::fulltext_index_key(relation_id, index_id))
            .map_err(CassieError::from)?
        else {
            return Ok(None);
        };
        let metadata = codec::decode_metadata(&raw)?;
        if metadata.version != STATE_VERSION {
            return Err(CassieError::Parse(format!(
                "unsupported fulltext metadata version {}",
                metadata.version
            )));
        }
        let loaded = load_complete_fulltext_postings(&tx, relation_id, index_id)?;
        let document_prefix = Self::fulltext_document_stats_prefix(relation_id, index_id);
        let document_stats = collect_scan(
            tx.scan(&Query::new().prefix(document_prefix.clone().into()))
                .map_err(CassieError::from)?,
        )?
        .into_iter()
        .map(|(key, raw)| {
            let document_id = key_encoding::utf8_suffix_after_prefix(&key, &document_prefix)
                .ok_or_else(|| CassieError::Parse("invalid fulltext document key".to_string()))?;
            let stats = codec::decode_document_stats(&raw).map_err(|error| {
                CassieError::Parse(format!(
                    "invalid fulltext document statistics for '{document_id}': {error}"
                ))
            })?;
            Ok((document_id, stats))
        })
        .collect::<Result<BTreeMap<_, _>, CassieError>>()?;
        let term_records = layout::load_term_records(&tx, relation_id, index_id)?;
        validate_complete_artifact_counts(&metadata, &term_records, &loaded, &document_stats)?;
        Ok(Some(PersistedFulltextIndexState {
            built_generation: metadata.built_generation,
            total_documents: metadata.total_documents,
            documents_with_text: metadata.documents_with_text,
            average_document_length: layout::average_document_length(
                metadata.total_document_length,
                metadata.documents_with_text,
            ),
            total_document_length: metadata.total_document_length,
            analyzer: metadata.analyzer,
            document_stats,
            postings: loaded.postings,
        }))
    }

    /// Reads only the requested term postings and matching document statistics.
    ///
    /// # Errors
    ///
    /// Returns an error when metadata, postings, or document statistics are stale or corrupt.
    pub fn fulltext_candidate_stats(
        &self,
        collection: &str,
        index_name: &str,
        terms: &[String],
    ) -> Result<BTreeMap<String, PersistedFulltextDocumentStats>, CassieError> {
        self.fulltext_candidate_set(collection, index_name, terms)
            .map(|candidates| candidates.document_stats)
    }

    /// Reads requested term postings but fetches document statistics only for allowed candidates.
    ///
    /// # Errors
    ///
    /// Returns an error when metadata, postings, or requested document statistics are stale or
    /// corrupt.
    pub fn fulltext_candidate_stats_for_ids(
        &self,
        collection: &str,
        index_name: &str,
        terms: &[String],
        allowed_ids: &std::collections::BTreeSet<String>,
    ) -> Result<BTreeMap<String, PersistedFulltextDocumentStats>, CassieError> {
        self.fulltext_candidate_set_inner(collection, index_name, terms, Some(allowed_ids))
            .map(|candidates| candidates.document_stats)
    }

    /// Reads only requested posting blocks and point-reads statistics for their documents.
    ///
    /// # Errors
    ///
    /// Returns an error when metadata, postings, or document statistics are stale or corrupt.
    pub fn fulltext_candidate_set(
        &self,
        collection: &str,
        index_name: &str,
        terms: &[String],
    ) -> Result<PersistedFulltextCandidateSet, CassieError> {
        self.fulltext_candidate_set_inner(collection, index_name, terms, None)
    }

    fn fulltext_candidate_set_inner(
        &self,
        collection: &str,
        index_name: &str,
        terms: &[String],
        allowed_ids: Option<&std::collections::BTreeSet<String>>,
    ) -> Result<PersistedFulltextCandidateSet, CassieError> {
        let collection = self.canonical_collection_name(collection);
        let index = self
            .get_index(&collection, index_name)?
            .ok_or_else(|| CassieError::Execution("fulltext fallback:missing-index".to_string()))?;
        let (relation_id, index_id) = Self::fulltext_storage_ids(&index)?;
        let tx = self.begin_data_readonly_tx_for(&collection)?;
        let metadata_raw = tx
            .get(&Self::fulltext_index_key(relation_id, index_id))
            .map_err(CassieError::from)?
            .ok_or_else(|| CassieError::Execution("fulltext fallback:missing-index".to_string()))?;
        let metadata = codec::decode_metadata(&metadata_raw)?;
        if metadata.version != STATE_VERSION
            || metadata.built_generation != self.collection_generation(&collection)?
        {
            return Err(CassieError::Execution(
                "fulltext fallback:stale-generation".to_string(),
            ));
        }
        let loaded = load_candidate_postings(&tx, relation_id, index_id, terms, allowed_ids)?;
        let row_schema = self.row_schema(&collection)?;
        for id in &loaded.source_ids {
            if !self.fulltext_source_document_exists(&tx, &collection, &row_schema, id)? {
                return Err(CassieError::Execution(
                    "fulltext fallback:missing_candidate_row".to_string(),
                ));
            }
        }
        let mut stats = BTreeMap::new();
        for id in loaded.ids {
            let Some(raw) = tx
                .get(&Self::fulltext_document_stats_key(
                    relation_id,
                    index_id,
                    &id,
                ))
                .map_err(CassieError::from)?
            else {
                return Err(CassieError::Execution(
                    "fulltext fallback:missing-document-stats".to_string(),
                ));
            };
            let document = codec::decode_document_stats(&raw)?;
            stats.insert(id, document);
        }
        Ok(PersistedFulltextCandidateSet {
            total_documents: metadata.total_documents,
            average_document_length: layout::average_document_length(
                metadata.total_document_length,
                metadata.documents_with_text,
            ),
            analyzer: metadata.analyzer,
            document_frequency: loaded.document_frequency,
            document_stats: stats,
            posting_block_reads: loaded.block_reads,
        })
    }

    fn fulltext_source_document_exists(
        &self,
        tx: &crate::adapter::AdapterTransaction,
        collection: &str,
        row_schema: &RowSchema,
        id: &str,
    ) -> Result<bool, CassieError> {
        if self.collection_uses_column_store(collection)? {
            return Self::load_column_store_document_from_tx(tx, collection, id, row_schema)
                .map(|document| document.is_some());
        }
        if tx
            .get(&Self::row_key(row_schema.relation_id, id))
            .map_err(CassieError::from)?
            .is_some()
        {
            return Ok(true);
        }
        tx.get(&Self::doc_key(collection, id))
            .map(|document| document.is_some())
            .map_err(CassieError::from)
    }

    fn load_documents_from_tx(
        &self,
        tx: &crate::adapter::AdapterTransaction,
        collection: &str,
        row_schema: &RowSchema,
    ) -> Result<Vec<(String, serde_json::Value)>, CassieError> {
        let mut documents = BTreeMap::new();
        let uses_column_store = self.collection_uses_column_store(collection)?;
        if uses_column_store {
            self.validate_column_store_field_markers(tx, collection, row_schema, || Ok(()))?;
            let prefix = Self::column_store_row_prefix(row_schema.relation_id);
            for (key, _) in collect_scan(
                tx.scan(&Query::new().prefix(prefix.clone().into()))
                    .map_err(CassieError::from)?,
            )? {
                let Some(id) = key_encoding::utf8_suffix_after_prefix(&key, &prefix) else {
                    continue;
                };
                if let Some(payload) =
                    Self::load_column_store_document_from_tx(tx, collection, &id, row_schema)?
                {
                    documents.insert(id, payload);
                }
            }
            return Ok(documents.into_iter().collect());
        }

        for prefix in [
            Self::row_prefix(row_schema.relation_id),
            Self::doc_prefix(collection),
        ] {
            for (key, raw) in collect_scan(
                tx.scan(&Query::new().prefix(prefix.clone().into()))
                    .map_err(CassieError::from)?,
            )? {
                let Some(id) = key_encoding::utf8_suffix_after_prefix(&key, &prefix) else {
                    continue;
                };
                documents.entry(id).or_insert(decode_row_with_hooks(
                    row_schema,
                    &raw,
                    tx.value_hooks(),
                )?);
            }
        }
        Ok(documents.into_iter().collect())
    }

    /// # Errors
    ///
    /// Returns an error when the persisted full-text state is missing, corrupt, or unreadable.
    fn delete_fulltext_artifacts_in_tx(
        tx: &mut crate::adapter::AdapterTransaction,
        index: &IndexMeta,
    ) -> Result<usize, CassieError> {
        let (relation_id, index_id) = Self::fulltext_storage_ids(index)?;
        let prefix = Self::fulltext_index_artifact_prefix(relation_id, index_id);
        let entries = collect_scan(
            tx.scan(&Query::new().prefix(prefix.into()))
                .map_err(CassieError::from)?,
        )?;
        let deleted = entries.len();
        for (key, _) in entries {
            tx.delete(key).map_err(CassieError::from)?;
        }
        Ok(deleted)
    }

    fn fulltext_storage_ids(index: &IndexMeta) -> Result<(u64, u64), CassieError> {
        let relation_id = index.relation_id().ok_or_else(|| {
            CassieError::Parse(format!("index '{}' is missing its relation id", index.name))
        })?;
        let index_id = index.storage_id().ok_or_else(|| {
            CassieError::Parse(format!("index '{}' is missing its storage id", index.name))
        })?;
        Ok((relation_id, index_id))
    }
}

/// Analyzes one document's text into the persisted per-document statistics.
pub(crate) fn analyze_document_statistics(
    analyzer: &AnalyzerConfig,
    text: &str,
) -> PersistedFulltextDocumentStats {
    let mut term_positions = BTreeMap::<String, Vec<u32>>::new();
    for (position, term) in analyzer.analyze_positioned(text) {
        term_positions.entry(term).or_default().push(position);
    }
    let term_counts = term_positions
        .iter()
        .map(|(term, positions)| (term.clone(), positions.len()))
        .collect::<BTreeMap<_, _>>();
    let doc_length = term_counts.values().sum::<usize>();
    PersistedFulltextDocumentStats {
        doc_length,
        term_counts,
        term_positions,
    }
}

fn build_state(
    index: &IndexMeta,
    generation: u64,
    documents: Vec<(String, serde_json::Value)>,
) -> Result<PersistedFulltextIndexState, CassieError> {
    let analyzer =
        AnalyzerConfig::from_index_options(&index.options).map_err(CassieError::Unsupported)?;
    let mut document_stats = BTreeMap::new();
    let mut postings = BTreeMap::<String, Vec<PersistedFulltextPosting>>::new();
    let mut total_length = 0usize;
    let total_documents = documents.len();
    for (document_id, payload) in documents {
        let Some(text) = payload
            .as_object()
            .and_then(|fields| fields.get(&index.field))
            .and_then(serde_json::Value::as_str)
        else {
            continue;
        };
        let stats = analyze_document_statistics(&analyzer, text);
        total_length += stats.doc_length;
        for (term, term_frequency) in &stats.term_counts {
            postings
                .entry(term.clone())
                .or_default()
                .push(PersistedFulltextPosting {
                    document_id: document_id.clone(),
                    term_frequency: *term_frequency,
                });
        }
        document_stats.insert(document_id, stats);
    }
    let documents_with_text = document_stats.len();
    for values in postings.values_mut() {
        values.sort_by(|left, right| left.document_id.cmp(&right.document_id));
    }
    Ok(PersistedFulltextIndexState {
        built_generation: generation,
        total_documents,
        documents_with_text,
        average_document_length: layout::average_document_length(total_length, documents_with_text),
        total_document_length: total_length,
        analyzer,
        document_stats,
        postings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_bound_prepared_fulltext_index_publication_batches() {
        // Arrange
        let record_bytes = vec![16_usize; 10_001];

        // Act
        let batches = fulltext_build_ranges(&record_bytes)
            .into_iter()
            .map(|range| range.len())
            .collect::<Vec<_>>();

        // Assert
        assert_eq!(batches, vec![5_000, 5_000, 1]);
    }

    #[test]
    fn should_bound_index_publication_batches_by_bytes_given_large_posting_blocks() {
        // Arrange
        let block = 48 * 1024;
        let oversized = FULLTEXT_INDEX_BUILD_WRITE_BATCH_BYTES + 1;
        let record_bytes = [vec![block; 50], vec![oversized], vec![block; 3]].concat();

        // Act
        let ranges = fulltext_build_ranges(&record_bytes);

        // Assert
        let covered = ranges.iter().map(std::ops::Range::len).sum::<usize>();
        assert_eq!(covered, record_bytes.len());
        assert_eq!(ranges.first(), Some(&(0..21)));
        assert!(
            ranges.contains(&(50..51)),
            "an oversized record stands alone"
        );
        for range in &ranges {
            let bytes = record_bytes[range.clone()].iter().sum::<usize>();
            assert!(range.len() == 1 || bytes <= FULLTEXT_INDEX_BUILD_WRITE_BATCH_BYTES);
        }
    }
}
