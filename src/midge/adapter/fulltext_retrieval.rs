use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{
    collect_scan, decode_row, key_encoding, CassieError, IndexMeta, Midge, Query, RowSchema,
    WriteOptions,
};
use crate::search::analyzer::AnalyzerConfig;

const STATE_VERSION: u32 = 1;

impl Midge {
    pub(crate) fn fulltext_index_key(collection: &str, name: &str) -> Vec<u8> {
        key_encoding::fulltext_index_key(collection, name)
    }

    pub(crate) fn fulltext_index_collection_prefix(collection: &str) -> Vec<u8> {
        key_encoding::fulltext_index_collection_prefix(collection)
    }

    pub(crate) fn fulltext_index_artifact_prefix(collection: &str, name: &str) -> Vec<u8> {
        key_encoding::fulltext_index_artifact_prefix(collection, name)
    }

    pub(crate) fn fulltext_index_manifest_key(
        collection: &str,
        name: &str,
        generation: u64,
    ) -> Vec<u8> {
        key_encoding::fulltext_index_manifest_key(collection, name, generation)
    }

    pub(crate) fn fulltext_postings_prefix(collection: &str, name: &str) -> Vec<u8> {
        key_encoding::fulltext_postings_prefix(collection, name)
    }

    pub(crate) fn fulltext_term_postings_key(collection: &str, name: &str, term: &str) -> Vec<u8> {
        key_encoding::fulltext_term_postings_key(collection, name, term)
    }

    pub(crate) fn fulltext_document_stats_prefix(collection: &str, name: &str) -> Vec<u8> {
        key_encoding::fulltext_document_stats_prefix(collection, name)
    }

    pub(crate) fn fulltext_document_stats_key(collection: &str, name: &str, id: &str) -> Vec<u8> {
        key_encoding::fulltext_document_stats_key(collection, name, id)
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
    collection: String,
    index_name: String,
    field: String,
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
}

impl Midge {
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
        Self::delete_fulltext_artifacts_in_tx(tx, collection, &index.name)?;
        Self::save_fulltext_state_in_tx(tx, collection, index, &state)
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
        let tx = self.begin_data_readonly_tx_for(&collection)?;
        let Some(raw) = tx
            .get(&Self::fulltext_index_key(&collection, index_name))
            .map_err(CassieError::from)?
        else {
            return Ok(None);
        };
        let metadata: FulltextIndexMetadata = serde_json::from_slice(&raw)
            .map_err(|error| CassieError::Parse(format!("invalid fulltext metadata: {error}")))?;
        if metadata.version != STATE_VERSION {
            return Err(CassieError::Parse(format!(
                "unsupported fulltext metadata version {}",
                metadata.version
            )));
        }
        let manifest_raw = tx
            .get(&Self::fulltext_index_manifest_key(
                &collection,
                index_name,
                metadata.built_generation,
            ))
            .map_err(CassieError::from)?
            .ok_or_else(|| {
                CassieError::Parse("missing fulltext generation manifest".to_string())
            })?;
        let manifest: FulltextManifest =
            serde_json::from_slice(&manifest_raw).map_err(|error| {
                CassieError::Parse(format!("invalid fulltext generation manifest: {error}"))
            })?;
        if manifest.version != STATE_VERSION
            || manifest.built_generation != metadata.built_generation
            || manifest.total_documents != metadata.total_documents
        {
            return Err(CassieError::Parse(
                "fulltext generation manifest does not match metadata".to_string(),
            ));
        }
        let postings_prefix = Self::fulltext_postings_prefix(&collection, index_name);
        let postings = collect_scan(
            tx.scan(&Query::new().prefix(postings_prefix.clone().into()))
                .map_err(CassieError::from)?,
        )?
        .into_iter()
        .map(|(key, raw)| {
            let term = key_encoding::utf8_suffix_after_prefix(&key, &postings_prefix)
                .ok_or_else(|| CassieError::Parse("invalid fulltext posting key".to_string()))?;
            let postings = serde_json::from_slice(&raw).map_err(|error| {
                CassieError::Parse(format!("invalid fulltext posting for '{term}': {error}"))
            })?;
            Ok((term, postings))
        })
        .collect::<Result<BTreeMap<_, _>, CassieError>>()?;
        let document_prefix = Self::fulltext_document_stats_prefix(&collection, index_name);
        let document_stats = collect_scan(
            tx.scan(&Query::new().prefix(document_prefix.clone().into()))
                .map_err(CassieError::from)?,
        )?
        .into_iter()
        .map(|(key, raw)| {
            let document_id = key_encoding::utf8_suffix_after_prefix(&key, &document_prefix)
                .ok_or_else(|| CassieError::Parse("invalid fulltext document key".to_string()))?;
            let stats = serde_json::from_slice(&raw).map_err(|error| {
                CassieError::Parse(format!(
                    "invalid fulltext document statistics for '{document_id}': {error}"
                ))
            })?;
            Ok((document_id, stats))
        })
        .collect::<Result<BTreeMap<_, _>, CassieError>>()?;
        Ok(Some(PersistedFulltextIndexState {
            built_generation: metadata.built_generation,
            total_documents: metadata.total_documents,
            documents_with_text: metadata.documents_with_text,
            average_document_length: metadata.average_document_length,
            analyzer: metadata.analyzer,
            document_stats,
            postings,
        }))
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
            let prefix = Self::column_store_row_prefix(collection);
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

        for prefix in [Self::row_prefix(collection), Self::doc_prefix(collection)] {
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
        collection: &str,
        index_name: &str,
    ) -> Result<(), CassieError> {
        let prefix = Self::fulltext_index_artifact_prefix(collection, index_name);
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
        collection: &str,
        index: &IndexMeta,
        state: &PersistedFulltextIndexState,
    ) -> Result<(), CassieError> {
        let metadata = FulltextIndexMetadata {
            version: STATE_VERSION,
            collection: collection.to_string(),
            index_name: index.name.clone(),
            field: index.field.clone(),
            built_generation: state.built_generation,
            total_documents: state.total_documents,
            documents_with_text: state.documents_with_text,
            average_document_length: state.average_document_length,
            analyzer: state.analyzer.clone(),
        };
        tx.put(
            Self::fulltext_index_key(collection, &index.name),
            serde_json::to_vec(&metadata).map_err(|error| CassieError::Parse(error.to_string()))?,
            None,
        )
        .map_err(CassieError::from)?;
        let manifest = FulltextManifest {
            version: STATE_VERSION,
            built_generation: state.built_generation,
            total_documents: state.total_documents,
            posting_terms: state.postings.len(),
            document_count: state.document_stats.len(),
        };
        tx.put(
            Self::fulltext_index_manifest_key(collection, &index.name, state.built_generation),
            serde_json::to_vec(&manifest).map_err(|error| CassieError::Parse(error.to_string()))?,
            None,
        )
        .map_err(CassieError::from)?;
        for (term, postings) in &state.postings {
            tx.put(
                Self::fulltext_term_postings_key(collection, &index.name, term),
                serde_json::to_vec(postings)
                    .map_err(|error| CassieError::Parse(error.to_string()))?,
                None,
            )
            .map_err(CassieError::from)?;
        }
        for (document_id, stats) in &state.document_stats {
            tx.put(
                Self::fulltext_document_stats_key(collection, &index.name, document_id),
                serde_json::to_vec(stats).map_err(|error| CassieError::Parse(error.to_string()))?,
                None,
            )
            .map_err(CassieError::from)?;
        }
        Ok(())
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
