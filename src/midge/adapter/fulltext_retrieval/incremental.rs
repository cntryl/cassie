//! Localized full-text maintenance for committed document writes.
//!
//! A commit that changes documents rewrites only: each changed document's
//! statistics record, the posting block that owns the document for every term it
//! gained, lost or changed frequency in, those terms' count records (when a
//! count moved), and the constant-size metadata record. Nothing proportional to
//! the unrelated corpus or vocabulary is read for writing or rewritten.
//!
//! All writes are computed first and staged; a stored index that contradicts the
//! changes (or is stale, or analyzed differently) is repaired by a full rebuild
//! inside the same transaction instead of failing the user's write.

use std::collections::{BTreeMap, BTreeSet};

use super::blocks::{self, PostingChange};
use super::layout::{document_length, document_terms, read_term_record, IndexRecord};
use super::metrics::FulltextWriteTally;
use super::{
    analyze_document_statistics, codec, collect_scan, decode_row, key_encoding, CassieError,
    FulltextIndexMetadata, FulltextTermRecord, IndexMeta, Midge, PersistedFulltextDocumentStats,
    PersistedFulltextPosting, Query, STATE_VERSION,
};
use crate::search::analyzer::AnalyzerConfig;

/// Document changes committed by one write batch.
pub(crate) struct FulltextCommitChanges<'a> {
    pub(crate) changed_ids: &'a [String],
    pub(crate) row_delta: i64,
}

#[derive(Default)]
struct StagedIndexWrites {
    puts: Vec<IndexRecord>,
    deletes: Vec<Vec<u8>>,
    blocks_written: usize,
    terms_touched: usize,
}

impl StagedIndexWrites {
    fn apply(
        mut self,
        tx: &mut crate::midge::adapter::AdapterTransaction,
        documents: usize,
    ) -> Result<FulltextWriteTally, CassieError> {
        let rewritten = self
            .puts
            .iter()
            .map(|(key, _)| key.clone())
            .collect::<BTreeSet<_>>();
        self.deletes.retain(|key| !rewritten.contains(key));
        let mut tally = FulltextWriteTally {
            documents,
            pieces_deleted: self.deletes.len(),
            pieces_written: self.puts.len(),
            terms_touched: self.terms_touched,
            blocks_written: self.blocks_written,
            ..FulltextWriteTally::default()
        };
        for key in self.deletes {
            tx.delete(key).map_err(CassieError::from)?;
        }
        for (key, value) in self.puts {
            tally.bytes_written = tally
                .bytes_written
                .saturating_add(key.len().saturating_add(value.len()));
            tx.put(key, value, None).map_err(CassieError::from)?;
        }
        Ok(tally)
    }
}

struct StoredBlock {
    first_id: String,
    postings: Vec<PersistedFulltextPosting>,
}

struct UpperBlock {
    first_id: String,
    key: Vec<u8>,
    raw: Vec<u8>,
}

struct BlockGroup {
    block: Option<StoredBlock>,
    upper: Option<UpperBlock>,
    changes: Vec<PostingChange>,
}

impl BlockGroup {
    fn covers(&self, document_id: &str) -> bool {
        self.upper
            .as_ref()
            .is_none_or(|upper| document_id < upper.first_id.as_str())
    }
}

struct TermScope<'a> {
    tx: &'a crate::midge::adapter::AdapterTransaction,
    relation_id: u64,
    index_id: u64,
    term: &'a str,
    prefix: Vec<u8>,
}

impl TermScope<'_> {
    fn first_id_of(&self, key: &[u8]) -> Result<String, CassieError> {
        key_encoding::utf8_suffix_after_prefix(key, &self.prefix)
            .ok_or_else(|| CassieError::Parse("invalid fulltext posting key".to_string()))
    }

    fn block_entry(&self, query: &Query) -> Result<Option<IndexRecord>, CassieError> {
        let entries = collect_scan(self.tx.scan(query).map_err(CassieError::from)?)?;
        Ok(entries
            .into_iter()
            .find(|(key, _)| key.starts_with(&self.prefix)))
    }

    fn stored_block(&self, key: &[u8], raw: &[u8]) -> Result<StoredBlock, CassieError> {
        let postings = codec::decode_postings(raw)?;
        let first_id = self.first_id_of(key)?;
        if postings.first().map(|posting| posting.document_id.as_str()) != Some(first_id.as_str()) {
            return Err(CassieError::Parse(format!(
                "misplaced fulltext posting block for '{}'",
                self.term
            )));
        }
        Ok(StoredBlock { first_id, postings })
    }

    /// Finds the block owning `document_id`: the last block starting at or before
    /// it, or the term's first block when the id sorts before every block.
    fn locate(&self, document_id: &str) -> Result<BlockGroup, CassieError> {
        let mut end = Midge::fulltext_term_postings_block_key(
            self.relation_id,
            self.index_id,
            self.term,
            document_id,
        );
        end.push(0);
        let at_or_before = self.block_entry(
            &Query::new()
                .start_key(self.prefix.clone().into())
                .end_key(end.into())
                .reverse()
                .limit(1),
        )?;
        let owner = match at_or_before {
            Some(entry) => Some(entry),
            None => self.block_entry(&Query::new().prefix(self.prefix.clone().into()).limit(1))?,
        };
        let Some((key, raw)) = owner else {
            return Ok(BlockGroup {
                block: None,
                upper: None,
                changes: Vec::new(),
            });
        };
        let block = self.stored_block(&key, &raw)?;
        let mut after = key;
        after.push(0);
        let upper = self
            .block_entry(
                &Query::new()
                    .prefix(self.prefix.clone().into())
                    .start_key(after.into())
                    .limit(1),
            )?
            .map(|(key, raw)| {
                Ok::<_, CassieError>(UpperBlock {
                    first_id: self.first_id_of(&key)?,
                    key,
                    raw,
                })
            })
            .transpose()?;
        Ok(BlockGroup {
            block: Some(block),
            upper,
            changes: Vec::new(),
        })
    }

    fn groups(&self, changes: Vec<PostingChange>) -> Result<Vec<BlockGroup>, CassieError> {
        let mut groups = Vec::<BlockGroup>::new();
        for change in changes {
            if groups
                .last()
                .is_none_or(|group| !group.covers(&change.document_id))
            {
                groups.push(self.locate(&change.document_id)?);
            }
            if let Some(group) = groups.last_mut() {
                group.changes.push(change);
            }
        }
        Ok(groups)
    }
}

struct TermPlan<'a> {
    relation_id: u64,
    index_id: u64,
    term: &'a str,
    staged: &'a mut StagedIndexWrites,
}

impl TermPlan<'_> {
    fn block_key(&self, first_id: &str) -> Vec<u8> {
        Midge::fulltext_term_postings_block_key(
            self.relation_id,
            self.index_id,
            self.term,
            first_id,
        )
    }

    /// Stages the rewrite of one block group and returns `(posting delta, block delta)`.
    fn stage_group(
        &mut self,
        group: BlockGroup,
        next_group_first: Option<&str>,
    ) -> Result<(isize, isize), CassieError> {
        let existed = group.block.is_some();
        let old_first = group.block.as_ref().map(|block| block.first_id.clone());
        let mut postings = group.block.map_or_else(Vec::new, |block| block.postings);
        let posting_delta = blocks::apply_changes(&mut postings, &group.changes)?;
        let mut block_delta = -isize::from(existed);
        if let Some(upper) = group
            .upper
            .filter(|upper| next_group_first != Some(upper.first_id.as_str()))
        {
            let own_bytes = codec::encoded_postings_len(&postings);
            if !postings.is_empty() && blocks::should_merge(own_bytes, upper.raw.len()) {
                postings.extend(codec::decode_postings(&upper.raw)?);
                self.staged.deletes.push(upper.key);
                block_delta -= 1;
            }
        }
        if let Some(old_first) = old_first {
            self.staged.deletes.push(self.block_key(&old_first));
        }
        for chunk in blocks::repack(&postings) {
            self.staged.puts.push((
                self.block_key(&chunk[0].document_id),
                codec::encode_postings(chunk)?,
            ));
            self.staged.blocks_written += 1;
            block_delta += 1;
        }
        Ok((posting_delta, block_delta))
    }
}

impl Midge {
    /// Maintains one full-text index for a commit's changed documents inside the
    /// commit transaction. Falls back to a full rebuild when the stored index is
    /// stale, was built with another analyzer, or contradicts the changes.
    pub(crate) fn maintain_fulltext_index_in_tx(
        &self,
        tx: &mut crate::midge::adapter::AdapterTransaction,
        collection: &str,
        index: &IndexMeta,
        generation: u64,
        changes: &FulltextCommitChanges<'_>,
    ) -> Result<FulltextWriteTally, CassieError> {
        // Any failure while planning (stale or contradictory stored state, corrupt
        // records) is repaired by the rebuild below; nothing is staged on error.
        let staged = self
            .stage_incremental_fulltext_writes(tx, collection, index, generation, changes)
            .ok()
            .flatten();
        match staged {
            Some(staged) => staged.apply(tx, changes.changed_ids.len()),
            None => self.rebuild_fulltext_index_in_tx(tx, collection, index, generation),
        }
    }

    fn load_fulltext_document_from_tx(
        &self,
        tx: &crate::midge::adapter::AdapterTransaction,
        collection: &str,
        row_schema: &crate::midge::row_blob::RowSchema,
        id: &str,
    ) -> Result<Option<serde_json::Value>, CassieError> {
        if self.collection_uses_column_store(collection)? {
            return Self::load_column_store_document_from_tx(tx, collection, id, row_schema);
        }
        let raw = match tx
            .get(&Self::row_key(row_schema.relation_id, id))
            .map_err(CassieError::from)?
        {
            Some(raw) => Some(raw),
            None => tx
                .get(&Self::doc_key(collection, id))
                .map_err(CassieError::from)?,
        };
        raw.map(|raw| decode_row(row_schema, &raw)).transpose()
    }

    fn stage_incremental_fulltext_writes(
        &self,
        tx: &crate::midge::adapter::AdapterTransaction,
        collection: &str,
        index: &IndexMeta,
        generation: u64,
        changes: &FulltextCommitChanges<'_>,
    ) -> Result<Option<StagedIndexWrites>, CassieError> {
        let (relation_id, index_id) = Self::fulltext_storage_ids(index)?;
        let analyzer =
            AnalyzerConfig::from_index_options(&index.options).map_err(CassieError::Unsupported)?;
        let metadata_key = Self::fulltext_index_key(relation_id, index_id);
        let Some(raw) = tx.get(&metadata_key).map_err(CassieError::from)? else {
            return Ok(None);
        };
        let mut metadata = codec::decode_metadata(&raw)?;
        if metadata.version != STATE_VERSION
            || metadata.built_generation != generation.wrapping_sub(1)
            || metadata.analyzer != analyzer
        {
            return Ok(None);
        }
        let row_schema = self.row_schema(collection)?;
        let changed_ids = changes.changed_ids.iter().collect::<BTreeSet<_>>();
        let mut staged = StagedIndexWrites::default();
        let mut term_changes = BTreeMap::<String, Vec<PostingChange>>::new();
        for id in changed_ids {
            let old = tx
                .get(&Self::fulltext_document_stats_key(
                    relation_id,
                    index_id,
                    id,
                ))
                .map_err(CassieError::from)?
                .map(|raw| codec::decode_document_stats(&raw))
                .transpose()?;
            let new = self
                .load_fulltext_document_from_tx(tx, collection, &row_schema, id)?
                .as_ref()
                .and_then(|payload| payload.as_object())
                .and_then(|fields| fields.get(&index.field))
                .and_then(serde_json::Value::as_str)
                .map(|text| analyze_document_statistics(&analyzer, text));
            stage_document(
                &mut staged,
                &mut term_changes,
                &mut metadata,
                (relation_id, index_id),
                id,
                old.as_ref(),
                new.as_ref(),
            )?;
        }
        for (term, term_changes) in term_changes {
            let terms_delta =
                stage_term(tx, &mut staged, relation_id, index_id, &term, term_changes)?;
            metadata.posting_terms = metadata
                .posting_terms
                .checked_add_signed(terms_delta)
                .ok_or_else(|| CassieError::Parse("fulltext term count underflow".to_string()))?;
            staged.terms_touched += 1;
        }
        metadata.total_documents = metadata
            .total_documents
            .checked_add_signed(isize::try_from(changes.row_delta).unwrap_or(isize::MAX))
            .filter(|total| *total >= metadata.documents_with_text)
            .ok_or_else(|| CassieError::Parse("fulltext document count underflow".to_string()))?;
        metadata.built_generation = generation;
        staged
            .puts
            .push((metadata_key, codec::encode_metadata(&metadata)));
        Ok(Some(staged))
    }
}

fn stage_document(
    staged: &mut StagedIndexWrites,
    term_changes: &mut BTreeMap<String, Vec<PostingChange>>,
    metadata: &mut FulltextIndexMetadata,
    (relation_id, index_id): (u64, u64),
    id: &str,
    old: Option<&PersistedFulltextDocumentStats>,
    new: Option<&PersistedFulltextDocumentStats>,
) -> Result<(), CassieError> {
    if old == new {
        return Ok(());
    }
    let old_terms = old.map_or_else(BTreeMap::new, |stats| {
        document_terms(stats).collect::<BTreeMap<_, _>>()
    });
    let new_terms = new.map_or_else(BTreeMap::new, |stats| {
        document_terms(stats).collect::<BTreeMap<_, _>>()
    });
    for term in old_terms
        .keys()
        .chain(new_terms.keys())
        .collect::<BTreeSet<_>>()
    {
        let (before, after) = (old_terms.get(term).copied(), new_terms.get(term).copied());
        if before != after {
            term_changes
                .entry((*term).to_string())
                .or_default()
                .push(PostingChange {
                    document_id: id.to_string(),
                    old: before,
                    new: after,
                });
        }
    }
    let key = Midge::fulltext_document_stats_key(relation_id, index_id, id);
    match new {
        Some(stats) => staged.puts.push((key, codec::encode_document_stats(stats))),
        None => staged.deletes.push(key),
    }
    let underflow = || CassieError::Parse("fulltext corpus totals underflow".to_string());
    metadata.documents_with_text = (metadata.documents_with_text + usize::from(new.is_some()))
        .checked_sub(usize::from(old.is_some()))
        .ok_or_else(underflow)?;
    metadata.total_document_length = (metadata.total_document_length
        + new.map_or(0, document_length))
    .checked_sub(old.map_or(0, document_length))
    .ok_or_else(underflow)?;
    Ok(())
}

/// Stages the block and term-record rewrites for one term and returns the change
/// in the number of indexed terms (`-1`, `0` or `1`).
fn stage_term(
    tx: &crate::midge::adapter::AdapterTransaction,
    staged: &mut StagedIndexWrites,
    relation_id: u64,
    index_id: u64,
    term: &str,
    changes: Vec<PostingChange>,
) -> Result<isize, CassieError> {
    let record = read_term_record(tx, relation_id, index_id, term)?;
    let scope = TermScope {
        tx,
        relation_id,
        index_id,
        term,
        prefix: Midge::fulltext_term_postings_prefix(relation_id, index_id, term),
    };
    let groups = scope.groups(changes)?;
    let next_firsts = groups
        .iter()
        .skip(1)
        .map(|group| group.block.as_ref().map(|block| block.first_id.clone()))
        .chain(std::iter::once(None))
        .collect::<Vec<_>>();
    let mut plan = TermPlan {
        relation_id,
        index_id,
        term,
        staged,
    };
    let mut posting_delta = 0_isize;
    let mut block_delta = 0_isize;
    for (group, next_first) in groups.into_iter().zip(next_firsts) {
        let (postings, blocks) = plan.stage_group(group, next_first.as_deref())?;
        posting_delta += postings;
        block_delta += blocks;
    }
    let inconsistent = || {
        CassieError::Parse(format!(
            "fulltext term record disagrees with postings for '{term}'"
        ))
    };
    let before = record.clone().unwrap_or(FulltextTermRecord {
        block_count: 0,
        posting_count: 0,
    });
    let after = FulltextTermRecord {
        block_count: before
            .block_count
            .checked_add_signed(block_delta)
            .ok_or_else(inconsistent)?,
        posting_count: before
            .posting_count
            .checked_add_signed(posting_delta)
            .ok_or_else(inconsistent)?,
    };
    if (after.posting_count == 0) != (after.block_count == 0) {
        return Err(inconsistent());
    }
    let key = Midge::fulltext_term_record_key(relation_id, index_id, term);
    if after.posting_count == 0 {
        if record.is_some() {
            staged.deletes.push(key);
        }
    } else if record.as_ref() != Some(&after) {
        staged.puts.push((key, codec::encode_term_record(&after)));
    }
    Ok(match (record.is_some(), after.posting_count > 0) {
        (false, true) => 1,
        (true, false) => -1,
        _ => 0,
    })
}
