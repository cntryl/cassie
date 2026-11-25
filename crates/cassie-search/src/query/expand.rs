//! Vocabulary expansion of prefix and typo items.

use super::limits::{MAX_EXPANDED_TERMS, MAX_EXPANSIONS_PER_ITEM};
use super::{invalid, limit_exceeded, ResolveError};

/// Which index terms a prefix or typo item stands for. Words are already
/// case- and accent-folded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TermMatcher {
    /// Terms starting with the word.
    Prefix(String),
    /// Terms within Levenshtein distance one of the word (the word included).
    OneEdit(String),
}

impl TermMatcher {
    #[must_use]
    pub fn matches(&self, term: &str) -> bool {
        match self {
            Self::Prefix(prefix) => term.starts_with(prefix.as_str()),
            Self::OneEdit(word) => within_one_edit(word, term),
        }
    }

    pub(super) fn describe(&self) -> String {
        match self {
            Self::Prefix(prefix) => format!("prefix '{prefix}*'"),
            Self::OneEdit(word) => format!("typo '{word}~'"),
        }
    }
}

/// The index terms of one field, as seen by one execution path.
///
/// This is the only way query resolution enumerates a vocabulary. Implementors
/// return, for each matcher, the sorted matching terms and stop collecting once
/// more than `limit` terms matched for that matcher (so a caller can tell
/// that the limit was exceeded without the list being truncated silently).
pub trait Vocabulary {
    /// The original error returned by this vocabulary implementation.
    type Error;

    /// # Errors
    ///
    /// Returns an error when the vocabulary cannot be read.
    fn matching_terms(
        &self,
        matchers: &[TermMatcher],
        limit: usize,
    ) -> Result<Vec<Vec<String>>, Self::Error>;
}

/// Matches `matchers` against `terms` in one pass; the shared implementation of
/// [`Vocabulary::matching_terms`].
#[must_use]
pub fn matching_terms_in<'a>(
    terms: impl IntoIterator<Item = &'a str>,
    matchers: &[TermMatcher],
    limit: usize,
) -> Vec<Vec<String>> {
    let mut matched = vec![Vec::<String>::new(); matchers.len()];
    for term in terms {
        for (matcher, found) in matchers.iter().zip(&mut matched) {
            if found.len() <= limit && matcher.matches(term) {
                found.push(term.to_string());
            }
        }
    }
    for found in &mut matched {
        found.sort();
        found.dedup();
    }
    matched
}

/// Expands `matchers` against `vocabulary`, enforcing the expansion limits.
pub(super) fn expand<V: Vocabulary + ?Sized>(
    vocabulary: &V,
    matchers: &[TermMatcher],
) -> Result<Vec<Vec<String>>, ResolveError<V::Error>> {
    if matchers.is_empty() {
        return Ok(Vec::new());
    }
    let expanded = vocabulary
        .matching_terms(matchers, MAX_EXPANSIONS_PER_ITEM)
        .map_err(ResolveError::Vocabulary)?;
    if expanded.len() != matchers.len() {
        return Err(ResolveError::Query(invalid(
            "full-text vocabulary returned a malformed expansion",
        )));
    }
    let mut total = 0usize;
    for (matcher, terms) in matchers.iter().zip(&expanded) {
        if terms.len() > MAX_EXPANSIONS_PER_ITEM {
            return Err(ResolveError::Query(limit_exceeded(format!(
                "full-text {} matches more than {MAX_EXPANSIONS_PER_ITEM} index terms \
                 (limit max_expansions_per_item)",
                matcher.describe()
            ))));
        }
        total = total.saturating_add(terms.len());
    }
    if total > MAX_EXPANDED_TERMS {
        return Err(ResolveError::Query(limit_exceeded(format!(
            "full-text query expands to {total} index terms, more than {MAX_EXPANDED_TERMS} \
             (limit max_expanded_terms)"
        ))));
    }
    Ok(expanded)
}

fn within_one_edit(word: &str, term: &str) -> bool {
    let left: Vec<char> = word.chars().collect();
    let right: Vec<char> = term.chars().collect();
    if left.len().abs_diff(right.len()) > 1 {
        return false;
    }
    let common = left
        .iter()
        .zip(&right)
        .take_while(|(left, right)| left == right)
        .count();
    let (left, right) = (&left[common..], &right[common..]);
    match left.len().cmp(&right.len()) {
        std::cmp::Ordering::Equal => left.len() <= 1 || left[1..] == right[1..],
        std::cmp::Ordering::Greater => left[1..] == right[..],
        std::cmp::Ordering::Less => left[..] == right[1..],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_match_terms_within_one_edit_given_insertion_deletion_and_substitution() {
        // Arrange
        let word = "color";

        // Act
        let results = [
            "color", "colour", "colr", "colon", "cooler", "clr", "colors",
        ]
        .map(|term| within_one_edit(word, term));

        // Assert
        assert_eq!(results, [true, true, true, true, false, false, true]);
    }

    #[test]
    fn should_not_match_a_transposition_given_distance_two() {
        // Arrange
        let word = "recieve";

        // Act
        let matched = within_one_edit(word, "receive");

        // Assert
        assert!(!matched);
    }

    #[test]
    fn should_report_one_more_than_the_limit_given_an_excessive_prefix() {
        // Arrange
        let terms = ["pa", "pb", "pc", "pd", "xx"];
        let matchers = [TermMatcher::Prefix("p".to_string())];

        // Act
        let matched = matching_terms_in(terms, &matchers, 2);

        // Assert
        assert_eq!(matched[0].len(), 3, "collection stops one past the limit");
    }
}
