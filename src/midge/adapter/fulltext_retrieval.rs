use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{
    collect_scan, decode_row, key_encoding, CassieError, IndexMeta, Midge, Query, RowSchema,
    WriteOptions,
};
use crate::search::analyzer::AnalyzerConfig;

#[path = "fulltext_retrieval/codec.rs"]
mod codec;
#[path = "fulltext_retrieval/controlled.rs"]
mod controlled;

const STATE_VERSION: u32 = 2;

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

    pub(crate) fn fulltext_index_manifest_key(
        relation_id: u64,
        index_id: u64,
        generation: u64,
    ) -> Vec<u8> {
        key_encoding::fulltext_index_manifest_key(relation_id, index_id, generation)
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
        block: usize,
    ) -> Vec<u8> {
        key_encoding::fulltext_term_postings_block_key(relation_id, index_id, term, block)
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
    pub term_counts: BTreeMap<String, usize>,
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
    pub analyzer: AnalyzerConfig,
    pub document_stats: BTreeMap<String, PersistedFulltextDocumentStats>,
    pub postings: BTreeMap<String, Vec<PersistedFulltextPosting>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct FulltextIndexMetadata {
    version: u32,
    built_generation: u64,
    total_documents: usize,
    documents_with_text: usize,
    average_document_length: f64,
    analyzer: AnalyzerConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct FulltextManifest {
    version: u32,
    built_generation: u64,
    total_documents: usize,
    posting_terms: usize,
    document_count: usize,
    terms: BTreeMap<String, FulltextTermIntegrity>,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct FulltextTermIntegrity {
    block_count: usize,
    posting_count: usize,
}

fn validate_manifest_header(
    metadata: &FulltextIndexMetadata,
    manifest: &FulltextManifest,
) -> Result<(), CassieError> {
    if manifest.version != STATE_VERSION
        || manifest.built_generation != metadata.built_generation
        || manifest.total_documents != metadata.total_documents
        || manifest.posting_terms != manifest.terms.len()
        || manifest.document_count != metadata.documents_with_text
    {
        return Err(CassieError::Parse(
            "fulltext generation manifest does not match metadata".to_string(),
        ));
    }
    Ok(())
}

fn validate_complete_artifact_counts(
    metadata: &FulltextIndexMetadata,
    manifest: &FulltextManifest,
    block_counts: &BTreeMap<String, usize>,
    posting_counts: &BTreeMap<String, usize>,
    document_count: usize,
) -> Result<(), CassieError> {
    validate_manifest_header(metadata, manifest)?;
    if manifest.document_count != document_count
        || manifest.terms.iter().any(|(term, integrity)| {
            block_counts.get(term).copied().unwrap_or_default() != integrity.block_count
                || posting_counts.get(term).copied().unwrap_or_default() != integrity.posting_count
        })
        || block_counts
            .keys()
            .any(|term| !manifest.terms.contains_key(term))
    {
        return Err(CassieError::Parse(
            "fulltext persisted artifact counts are inconsistent".to_string(),
        ));
    }
    Ok(())
}

fn load_complete_fulltext_postings(
    tx: &cntryl_midge::Transaction,
    relation_id: u64,
    index_id: u64,
) -> Result<LoadedFulltextPostings, CassieError> {
    let postings_prefix = Midge::fulltext_postings_prefix(relation_id, index_id);
    let posting_entries = collect_scan(
        tx.scan(&Query::new().prefix(postings_prefix.clone().into()))
            .map_err(CassieError::from)?,
    )?;
    let mut postings = BTreeMap::<String, Vec<PersistedFulltextPosting>>::new();
    let mut block_counts = BTreeMap::<String, usize>::new();
    let mut posting_counts = BTreeMap::<String, usize>::new();
    let mut posting_ids = BTreeMap::<String, std::collections::BTreeSet<String>>::new();
    for (key, raw) in posting_entries {
        let term = key_encoding::utf8_first_component_after_prefix(&key, &postings_prefix)
            .ok_or_else(|| CassieError::Parse("invalid fulltext posting key".to_string()))?;
        let block = block_counts.entry(term.clone()).or_default();
        if key != Midge::fulltext_term_postings_block_key(relation_id, index_id, &term, *block) {
            return Err(CassieError::Parse(format!(
                "non-contiguous fulltext posting blocks for '{term}'"
            )));
        }
        *block = block.saturating_add(1);
        let block = codec::decode_postings(&raw).map_err(|error| {
            CassieError::Parse(format!("invalid fulltext posting for '{term}': {error}"))
        })?;
        let ids = posting_ids.entry(term.clone()).or_default();
        for posting in &block {
            if !ids.insert(posting.document_id.clone()) {
                return Err(CassieError::Parse(format!(
                    "duplicate fulltext posting for '{}' in '{term}'",
                    posting.document_id
                )));
            }
        }
        let count = posting_counts.entry(term.clone()).or_default();
        *count = count.saturating_add(block.len());
        postings.entry(term).or_default().extend(block);
    }
    Ok(LoadedFulltextPostings {
        postings,
        block_counts,
        posting_counts,
    })
}

fn load_candidate_postings(
    tx: &cntryl_midge::Transaction,
    relation_id: u64,
    index_id: u64,
    terms: &[String],
    allowed_ids: Option<&std::collections::BTreeSet<String>>,
    manifest: &FulltextManifest,
) -> Result<LoadedCandidatePostings, CassieError> {
    let mut loaded = LoadedCandidatePostings {
        ids: std::collections::BTreeSet::new(),
        source_ids: std::collections::BTreeSet::new(),
        document_frequency: BTreeMap::new(),
        block_reads: 0,
    };
    for term in terms {
        let prefix = Midge::fulltext_term_postings_prefix(relation_id, index_id, term);
        let entries = collect_scan(
            tx.scan(&Query::new().prefix(prefix.into()))
                .map_err(CassieError::from)?,
        )?;
        loaded.block_reads = loaded.block_reads.saturating_add(entries.len());
        let expected = manifest.terms.get(term);
        if entries.len() != expected.map_or(0, |integrity| integrity.block_count) {
            return Err(CassieError::Parse(format!(
                "incomplete fulltext posting blocks for '{term}'"
            )));
        }
        let mut term_documents = std::collections::BTreeSet::new();
        let mut posting_count = 0usize;
        for (block, (key, raw)) in entries.into_iter().enumerate() {
            if key != Midge::fulltext_term_postings_block_key(relation_id, index_id, term, block) {
                return Err(CassieError::Parse(format!(
                    "non-contiguous fulltext posting blocks for '{term}'"
                )));
            }
            let postings = codec::decode_postings(&raw)?;
            posting_count = posting_count.saturating_add(postings.len());
            for posting in postings {
                if !term_documents.insert(posting.document_id.clone()) {
                    return Err(CassieError::Parse(format!(
                        "duplicate fulltext posting for '{}' in '{term}'",
                        posting.document_id
                    )));
                }
                if allowed_ids.is_none_or(|allowed| allowed.contains(&posting.document_id)) {
                    loaded.ids.insert(posting.document_id);
                }
            }
        }
        if posting_count != expected.map_or(0, |integrity| integrity.posting_count) {
            return Err(CassieError::Parse(format!(
                "incomplete fulltext postings for '{term}'"
            )));
        }
        loaded.source_ids.extend(term_documents.iter().cloned());
        loaded
            .document_frequency
            .insert(term.clone(), term_documents.len());
    }
    Ok(loaded)
}

impl Midge {
    pub(crate) fn reconcile_fulltext_indexes(&self) -> Result<(), CassieError> {
        let indexes = self
            .list_indexes()?
            .into_iter()
            .filter(|index| index.kind == crate::catalog::IndexKind::FullText)
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

    pub(crate) fn rebuild_fulltext_index_for_index(
        &self,
        index: &IndexMeta,
    ) -> Result<(), CassieError> {
        let collection = self.canonical_collection_name(&index.collection);
        let generation = self.collection_generation(&collection)?;
        let mut tx = self.begin_data_rw_tx_for(&collection)?;
        self.rebuild_fulltext_index_in_tx(&mut tx, &collection, index, generation)?;
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)
    }

    pub(crate) fn rebuild_fulltext_index_in_tx(
        &self,
        tx: &mut cntryl_midge::Transaction,
        collection: &str,
        index: &IndexMeta,
        generation: u64,
    ) -> Result<(), CassieError> {
        let row_schema = self.row_schema(collection)?;
        let documents = self.load_documents_from_tx(tx, collection, &row_schema)?;
        let state = build_state(index, generation, documents)?;
        Self::delete_fulltext_artifacts_in_tx(tx, index)?;
        Self::save_fulltext_state_in_tx(tx, index, &state)
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
        let manifest_raw = tx
            .get(&Self::fulltext_index_manifest_key(
                relation_id,
                index_id,
                metadata.built_generation,
            ))
            .map_err(CassieError::from)?
            .ok_or_else(|| {
                CassieError::Parse("missing fulltext generation manifest".to_string())
            })?;
        let manifest = codec::decode_manifest(&manifest_raw)?;
        validate_manifest_header(&metadata, &manifest)?;
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
        validate_complete_artifact_counts(
            &metadata,
            &manifest,
            &loaded.block_counts,
            &loaded.posting_counts,
            document_stats.len(),
        )?;
        Ok(Some(PersistedFulltextIndexState {
            built_generation: metadata.built_generation,
            total_documents: metadata.total_documents,
            documents_with_text: metadata.documents_with_text,
            average_document_length: metadata.average_document_length,
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
        let manifest_raw = tx
            .get(&Self::fulltext_index_manifest_key(
                relation_id,
                index_id,
                metadata.built_generation,
            ))
            .map_err(CassieError::from)?
            .ok_or_else(|| {
                CassieError::Parse("missing fulltext generation manifest".to_string())
            })?;
        let manifest = codec::decode_manifest(&manifest_raw)?;
        validate_manifest_header(&metadata, &manifest)?;
        let loaded =
            load_candidate_postings(&tx, relation_id, index_id, terms, allowed_ids, &manifest)?;
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
            average_document_length: metadata.average_document_length,
            analyzer: metadata.analyzer,
            document_frequency: loaded.document_frequency,
            document_stats: stats,
            posting_block_reads: loaded.block_reads,
        })
    }

    fn fulltext_source_document_exists(
        &self,
        tx: &cntryl_midge::Transaction,
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
        tx: &cntryl_midge::Transaction,
        collection: &str,
        row_schema: &RowSchema,
    ) -> Result<Vec<(String, serde_json::Value)>, CassieError> {
        let mut documents = BTreeMap::new();
        let uses_column_store = self.collection_uses_column_store(collection)?;
        if uses_column_store {
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
                documents.entry(id).or_insert(decode_row(row_schema, &raw)?);
            }
        }
        Ok(documents.into_iter().collect())
    }

    /// # Errors
    ///
    /// Returns an error when the persisted full-text state is missing, corrupt, or unreadable.
    fn delete_fulltext_artifacts_in_tx(
        tx: &mut cntryl_midge::Transaction,
        index: &IndexMeta,
    ) -> Result<(), CassieError> {
        let (relation_id, index_id) = Self::fulltext_storage_ids(index)?;
        let prefix = Self::fulltext_index_artifact_prefix(relation_id, index_id);
        let entries = collect_scan(
            tx.scan(&Query::new().prefix(prefix.into()))
                .map_err(CassieError::from)?,
        )?;
        for (key, _) in entries {
            tx.delete(key).map_err(CassieError::from)?;
        }
        Ok(())
    }

    fn save_fulltext_state_in_tx(
        tx: &mut cntryl_midge::Transaction,
        index: &IndexMeta,
        state: &PersistedFulltextIndexState,
    ) -> Result<(), CassieError> {
        let (relation_id, index_id) = Self::fulltext_storage_ids(index)?;
        let metadata = FulltextIndexMetadata {
            version: STATE_VERSION,
            built_generation: state.built_generation,
            total_documents: state.total_documents,
            documents_with_text: state.documents_with_text,
            average_document_length: state.average_document_length,
            analyzer: state.analyzer.clone(),
        };
        tx.put(
            Self::fulltext_index_key(relation_id, index_id),
            codec::encode_metadata(&metadata),
            None,
        )
        .map_err(CassieError::from)?;
        let posting_blocks = state
            .postings
            .iter()
            .map(|(term, postings)| {
                codec::encode_posting_blocks(postings).map(|blocks| (term.clone(), blocks))
            })
            .collect::<Result<BTreeMap<_, _>, CassieError>>()?;
        let terms = posting_blocks
            .iter()
            .map(|(term, blocks)| {
                (
                    term.clone(),
                    FulltextTermIntegrity {
                        block_count: blocks.len(),
                        posting_count: state.postings.get(term).map_or(0, Vec::len),
                    },
                )
            })
            .collect();
        let manifest = FulltextManifest {
            version: STATE_VERSION,
            built_generation: state.built_generation,
            total_documents: state.total_documents,
            posting_terms: state.postings.len(),
            document_count: state.document_stats.len(),
            terms,
        };
        tx.put(
            Self::fulltext_index_manifest_key(relation_id, index_id, state.built_generation),
            codec::encode_manifest(&manifest),
            None,
        )
        .map_err(CassieError::from)?;
        for (term, blocks) in posting_blocks {
            for (block, encoded) in blocks.into_iter().enumerate() {
                tx.put(
                    Self::fulltext_term_postings_block_key(relation_id, index_id, &term, block),
                    encoded,
                    None,
                )
                .map_err(CassieError::from)?;
            }
        }
        for (document_id, stats) in &state.document_stats {
            tx.put(
                Self::fulltext_document_stats_key(relation_id, index_id, document_id),
                codec::encode_document_stats(stats),
                None,
            )
            .map_err(CassieError::from)?;
        }
        Ok(())
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
            .and_then(|fields| {
                fields
                    .iter()
                    .find(|(name, _)| name.eq_ignore_ascii_case(&index.field))
            })
            .and_then(|(_, value)| value.as_str())
        else {
            continue;
        };
        let tokens = analyzer.analyze(text);
        let mut counts = BTreeMap::new();
        for token in tokens {
            counts
                .entry(token)
                .and_modify(|count| *count += 1)
                .or_insert(1);
        }
        let doc_length = counts.values().sum::<usize>();
        total_length += doc_length;
        for (term, term_frequency) in &counts {
            postings
                .entry(term.clone())
                .or_default()
                .push(PersistedFulltextPosting {
                    document_id: document_id.clone(),
                    term_frequency: *term_frequency,
                });
        }
        document_stats.insert(
            document_id,
            PersistedFulltextDocumentStats {
                doc_length,
                term_counts: counts,
            },
        );
    }
    let documents_with_text = document_stats.len();
    for values in postings.values_mut() {
        values.sort_by(|left, right| left.document_id.cmp(&right.document_id));
    }
    Ok(PersistedFulltextIndexState {
        built_generation: generation,
        total_documents,
        documents_with_text,
        average_document_length: if documents_with_text == 0 {
            0.0
        } else {
            usize_to_f64(total_length) / usize_to_f64(documents_with_text)
        },
        analyzer,
        document_stats,
        postings,
    })
}

fn usize_to_f64(value: usize) -> f64 {
    value.to_string().parse::<f64>().unwrap_or(f64::INFINITY)
}
