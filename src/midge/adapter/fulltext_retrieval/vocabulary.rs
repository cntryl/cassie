//! The index vocabulary as seen by `'query'`-syntax prefix and typo expansion.

use cntryl_midge::Query;

use crate::app::CassieError;
use crate::midge::adapter::{key_encoding, AdapterTransaction};
use crate::runtime::QueryExecutionControls;
use crate::search::query::{TermMatcher, Vocabulary};

/// The persisted term directory of one full-text index, read inside the
/// transaction that also reads its postings and document statistics.
pub(super) struct TermDirectory<'a> {
    pub(super) tx: &'a AdapterTransaction,
    pub(super) relation_id: u64,
    pub(super) index_id: u64,
    pub(super) controls: &'a QueryExecutionControls,
}

/// The single place that enumerates an index's vocabulary for query
/// expansion. For each matcher it returns the sorted matching index terms and
/// stops collecting once more than `limit` terms matched, so callers can tell
/// that a limit was exceeded without silently truncating.
///
/// Everything that expands prefix or typo items against persisted terms goes
/// through this function. It streams the per-term records (never posting
/// blocks), honours cancellation and the deadline on every record, and
/// retains at most `limit + 1` terms per matcher.
pub(super) fn vocabulary_terms(
    directory: &TermDirectory<'_>,
    matchers: &[TermMatcher],
    limit: usize,
) -> Result<Vec<Vec<String>>, CassieError> {
    let prefix =
        key_encoding::fulltext_term_records_prefix(directory.relation_id, directory.index_id, "");
    let mut scan = directory
        .tx
        .scan(&Query::new().prefix(prefix.clone().into()))
        .map_err(CassieError::from)?;
    let mut matched = vec![Vec::<String>::new(); matchers.len()];
    for entry in &mut scan {
        if directory.controls.is_canceled() {
            return Err(CassieError::QueryCanceled);
        }
        if directory.controls.is_timed_out() {
            return Err(CassieError::DeadlineExceeded);
        }
        let (key, _) = entry.map_err(CassieError::from)?;
        let term = key_encoding::utf8_first_component_after_prefix(&key, &prefix)
            .ok_or_else(|| CassieError::Parse("invalid fulltext term key".to_string()))?;
        for (matcher, found) in matchers.iter().zip(&mut matched) {
            if found.len() <= limit && matcher.matches(&term) {
                found.push(term.clone());
            }
        }
    }
    for found in &mut matched {
        found.sort();
        found.dedup();
    }
    Ok(matched)
}

impl Vocabulary for TermDirectory<'_> {
    fn matching_terms(
        &self,
        matchers: &[TermMatcher],
        limit: usize,
    ) -> Result<Vec<Vec<String>>, CassieError> {
        vocabulary_terms(self, matchers, limit)
    }
}
