//! The index vocabulary as seen by `'query'`-syntax prefix and typo expansion.

use super::FulltextManifest;
use crate::app::CassieError;
use crate::search::query::{matching_terms_in, TermMatcher, Vocabulary};

/// The single place that enumerates an index's vocabulary for query
/// expansion. For each matcher it returns the sorted matching index terms and
/// stops collecting once more than `limit` terms matched, so callers can tell
/// that a limit was exceeded without silently truncating.
///
/// Everything that expands prefix or typo items against persisted terms goes
/// through this function; re-point it when the term directory moves.
pub(super) fn vocabulary_terms(
    manifest: &FulltextManifest,
    matchers: &[TermMatcher],
    limit: usize,
) -> Vec<Vec<String>> {
    matching_terms_in(
        manifest.terms.keys().map(String::as_str),
        matchers,
        limit,
    )
}

/// [`Vocabulary`] over a decoded generation manifest.
pub(super) struct ManifestVocabulary<'a>(pub(super) &'a FulltextManifest);

impl Vocabulary for ManifestVocabulary<'_> {
    fn matching_terms(
        &self,
        matchers: &[TermMatcher],
        limit: usize,
    ) -> Result<Vec<Vec<String>>, CassieError> {
        Ok(vocabulary_terms(self.0, matchers, limit))
    }
}
