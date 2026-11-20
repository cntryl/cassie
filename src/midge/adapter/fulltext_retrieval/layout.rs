//! Persisted full-text layout shared by rebuilds, incremental maintenance and readers.
//!
//! An index owns four kinds of records under its artifact prefix:
//!
//! * one metadata record (constant size) with the exact corpus totals;
//! * one term record per vocabulary term holding the term's block and posting counts;
//! * posting blocks per term, keyed by the document id of their first posting so
//!   one document's block is found with a single reverse seek;
//! * one statistics record per document with text.
//!
//! Readers validate every term they touch against its term record, so a missing or
//! damaged block is detected without loading unrelated vocabulary.

use std::collections::BTreeMap;

use super::{
    codec, collect_scan, key_encoding, CassieError, FulltextIndexMetadata, FulltextTermRecord,
    IndexMeta, Midge, PersistedFulltextIndexState, PersistedFulltextPosting, Query, STATE_VERSION,
};
use crate::types::numeric::usize_to_f64;

/// Fill level used when packing blocks during a full build, leaving room for
/// later in-place growth before a block has to split.
pub(super) const BUILD_BLOCK_TARGET_BYTES: usize = codec::MAX_POSTING_BLOCK_BYTES / 4 * 3;

pub(super) type IndexRecord = (Vec<u8>, Vec<u8>);

pub(super) fn average_document_length(total_document_length: usize, documents: usize) -> f64 {
    if documents == 0 {
        0.0
    } else {
        usize_to_f64(total_document_length) / usize_to_f64(documents)
    }
}

/// Terms and per-term frequencies of one document's statistics.
pub(super) fn document_terms(
    stats: &super::PersistedFulltextDocumentStats,
) -> impl Iterator<Item = (&str, usize)> {
    stats
        .term_counts
        .iter()
        .map(|(term, count)| (term.as_str(), *count))
}

/// Total token count of one document's statistics.
pub(super) fn document_length(stats: &super::PersistedFulltextDocumentStats) -> usize {
    stats.doc_length
}

/// The records of a complete index state, ready to be written.
pub(super) struct FulltextStateRecords {
    pub(super) metadata: IndexRecord,
    pub(super) body: Vec<IndexRecord>,
}

impl Midge {
    pub(super) fn fulltext_state_records(
        index: &IndexMeta,
        state: &PersistedFulltextIndexState,
    ) -> Result<FulltextStateRecords, CassieError> {
        let (relation_id, index_id) = Self::fulltext_storage_ids(index)?;
        let mut body = Vec::new();
        for (term, postings) in &state.postings {
            let chunks = codec::chunk_postings(postings, BUILD_BLOCK_TARGET_BYTES);
            for chunk in &chunks {
                body.push((
                    Self::fulltext_term_postings_block_key(
                        relation_id,
                        index_id,
                        term,
                        &chunk[0].document_id,
                    ),
                    codec::encode_postings(chunk)?,
                ));
            }
            body.push((
                Self::fulltext_term_record_key(relation_id, index_id, term),
                codec::encode_term_record(&FulltextTermRecord {
                    block_count: chunks.len(),
                    posting_count: postings.len(),
                }),
            ));
        }
        for (document_id, stats) in &state.document_stats {
            body.push((
                Self::fulltext_document_stats_key(relation_id, index_id, document_id),
                codec::encode_document_stats(stats),
            ));
        }
        let metadata = FulltextIndexMetadata {
            version: STATE_VERSION,
            built_generation: state.built_generation,
            total_documents: state.total_documents,
            documents_with_text: state.documents_with_text,
            total_document_length: state.total_document_length,
            posting_terms: state.postings.len(),
            analyzer: state.analyzer.clone(),
        };
        Ok(FulltextStateRecords {
            metadata: (
                Self::fulltext_index_key(relation_id, index_id),
                codec::encode_metadata(&metadata),
            ),
            body,
        })
    }

    /// Enumerates index terms starting with `term_prefix` in byte order from a
    /// read transaction, with each term's document frequency.
    ///
    /// Only the per-term records are scanned, never posting blocks, so cost is
    /// bounded by `limit`. An empty prefix enumerates the whole vocabulary.
    ///
    /// # Errors
    ///
    /// Returns an error when a term record is unreadable or corrupt.
    pub(crate) fn fulltext_index_terms_in_tx(
        tx: &crate::midge::adapter::AdapterTransaction,
        relation_id: u64,
        index_id: u64,
        term_prefix: &str,
        limit: usize,
    ) -> Result<Vec<(String, usize)>, CassieError> {
        let record_prefix = key_encoding::fulltext_term_records_prefix(relation_id, index_id, "");
        let scan_prefix =
            key_encoding::fulltext_term_records_prefix(relation_id, index_id, term_prefix);
        let scan = tx
            .scan(&Query::new().prefix(scan_prefix.into()).limit(limit))
            .map_err(CassieError::from)?;
        let mut terms = Vec::new();
        for entry in collect_scan(scan)? {
            let (key, raw) = entry;
            let term = key_encoding::utf8_first_component_after_prefix(&key, &record_prefix)
                .ok_or_else(|| CassieError::Parse("invalid fulltext term key".to_string()))?;
            terms.push((term, codec::decode_term_record(&raw)?.posting_count));
        }
        Ok(terms)
    }

    /// Enumerates index terms starting with `term_prefix`; see
    /// [`Self::fulltext_index_terms_in_tx`].
    ///
    /// # Errors
    ///
    /// Returns an error when the index is missing or its term records are corrupt.
    pub fn fulltext_index_terms(
        &self,
        collection: &str,
        index_name: &str,
        term_prefix: &str,
        limit: usize,
    ) -> Result<Vec<(String, usize)>, CassieError> {
        let collection = self.canonical_collection_name(collection);
        let index = self
            .get_index(&collection, index_name)?
            .ok_or_else(|| CassieError::Execution("fulltext fallback:missing-index".to_string()))?;
        let (relation_id, index_id) = Self::fulltext_storage_ids(&index)?;
        let tx = self.begin_data_readonly_tx_for(&collection)?;
        Self::fulltext_index_terms_in_tx(&tx, relation_id, index_id, term_prefix, limit)
    }
}

/// Checks one term's posting blocks, in key order, against its term record.
pub(super) struct TermBlockValidator<'a> {
    term: &'a str,
    relation_id: u64,
    index_id: u64,
    previous_last: Option<String>,
    block_count: usize,
    posting_count: usize,
}

impl<'a> TermBlockValidator<'a> {
    pub(super) fn new(term: &'a str, relation_id: u64, index_id: u64) -> Self {
        Self {
            term,
            relation_id,
            index_id,
            previous_last: None,
            block_count: 0,
            posting_count: 0,
        }
    }

    /// # Errors
    ///
    /// Returns an error when the block is empty, mis-keyed or overlaps a previous block.
    pub(super) fn accept(
        &mut self,
        key: &[u8],
        postings: &[PersistedFulltextPosting],
    ) -> Result<(), CassieError> {
        let term = self.term;
        let (Some(first), Some(last)) = (postings.first(), postings.last()) else {
            return Err(CassieError::Parse(format!(
                "empty fulltext posting block for '{term}'"
            )));
        };
        if key
            != Midge::fulltext_term_postings_block_key(
                self.relation_id,
                self.index_id,
                term,
                &first.document_id,
            )
        {
            return Err(CassieError::Parse(format!(
                "misplaced fulltext posting block for '{term}'"
            )));
        }
        if postings
            .windows(2)
            .any(|pair| pair[0].document_id >= pair[1].document_id)
            || self
                .previous_last
                .as_ref()
                .is_some_and(|previous| *previous >= first.document_id)
        {
            return Err(CassieError::Parse(format!(
                "duplicate fulltext posting for '{}' in '{term}'",
                first.document_id
            )));
        }
        self.previous_last = Some(last.document_id.clone());
        self.block_count = self.block_count.saturating_add(1);
        self.posting_count = self.posting_count.saturating_add(postings.len());
        Ok(())
    }

    pub(super) fn block_count(&self) -> usize {
        self.block_count
    }

    pub(super) fn posting_count(&self) -> usize {
        self.posting_count
    }

    /// # Errors
    ///
    /// Returns an error when the observed blocks disagree with the term record.
    pub(super) fn finish(self, record: Option<&FulltextTermRecord>) -> Result<(), CassieError> {
        let term = self.term;
        let expected_blocks = record.map_or(0, |record| record.block_count);
        let expected_postings = record.map_or(0, |record| record.posting_count);
        if self.block_count != expected_blocks {
            return Err(CassieError::Parse(format!(
                "incomplete fulltext posting blocks for '{term}'"
            )));
        }
        if self.posting_count != expected_postings {
            return Err(CassieError::Parse(format!(
                "incomplete fulltext postings for '{term}'"
            )));
        }
        Ok(())
    }
}

/// Reads and decodes the term record for `term`, if any.
pub(super) fn read_term_record(
    tx: &crate::midge::adapter::AdapterTransaction,
    relation_id: u64,
    index_id: u64,
    term: &str,
) -> Result<Option<FulltextTermRecord>, CassieError> {
    tx.get(&Midge::fulltext_term_record_key(
        relation_id,
        index_id,
        term,
    ))
    .map_err(CassieError::from)?
    .map(|raw| codec::decode_term_record(&raw))
    .transpose()
}

/// All term records of an index, keyed by term.
pub(super) fn load_term_records(
    tx: &crate::midge::adapter::AdapterTransaction,
    relation_id: u64,
    index_id: u64,
) -> Result<BTreeMap<String, FulltextTermRecord>, CassieError> {
    let prefix = key_encoding::fulltext_term_records_prefix(relation_id, index_id, "");
    collect_scan(
        tx.scan(&Query::new().prefix(prefix.clone().into()))
            .map_err(CassieError::from)?,
    )?
    .into_iter()
    .map(|(key, raw)| {
        let term = key_encoding::utf8_first_component_after_prefix(&key, &prefix)
            .ok_or_else(|| CassieError::Parse("invalid fulltext term key".to_string()))?;
        Ok((term, codec::decode_term_record(&raw)?))
    })
    .collect()
}
