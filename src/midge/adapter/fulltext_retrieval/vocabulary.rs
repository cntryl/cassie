//! The index vocabulary as seen by `'query'`-syntax prefix and typo expansion.

use crate::app::CassieError;
use crate::midge::adapter::{AdapterTransaction, Midge};
use crate::search::query::{matching_terms_in, TermMatcher, Vocabulary};

/// The persisted term directory of one full-text index, read inside the
/// transaction that also reads its postings and document statistics.
pub(super) struct TermDirectory<'a> {
    pub(super) tx: &'a AdapterTransaction,
    pub(super) relation_id: u64,
    pub(super) index_id: u64,
}

/// The single place that enumerates an index's vocabulary for query
/// expansion. For each matcher it returns the sorted matching index terms and
/// stops collecting once more than `limit` terms matched, so callers can tell
/// that a limit was exceeded without silently truncating.
///
/// Everything that expands prefix or typo items against persisted terms goes
/// through this function. It scans the per-term records only, never posting
/// blocks.
pub(super) fn vocabulary_terms(
    directory: &TermDirectory<'_>,
    matchers: &[TermMatcher],
    limit: usize,
) -> Result<Vec<Vec<String>>, CassieError> {
    let terms = Midge::fulltext_index_terms_in_tx(
        directory.tx,
        directory.relation_id,
        directory.index_id,
        "",
        usize::MAX,
    )?;
    Ok(matching_terms_in(
        terms.iter().map(|(term, _)| term.as_str()),
        matchers,
        limit,
    ))
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
