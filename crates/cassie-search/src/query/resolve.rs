//! Vocabulary resolution: prefix and typo items become explicit term lists.

use super::compile::{node_bytes, CompiledQuery, Node, PhraseLeaf};
use super::expand::{expand, TermMatcher, Vocabulary};
use std::fmt::{Display, Formatter};

use super::SearchQueryError;

/// Resolution can fail while parsing query semantics or reading its vocabulary.
#[derive(Debug, PartialEq, Eq)]
pub enum ResolveError<E> {
    /// Query compilation or expansion rejected the query.
    Query(SearchQueryError),
    /// The vocabulary returned its original error unchanged.
    Vocabulary(E),
}

impl<E: Display> Display for ResolveError<E> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Query(error) => Display::fmt(error, formatter),
            Self::Vocabulary(error) => Display::fmt(error, formatter),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for ResolveError<E> {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Leaf {
    Phrase(PhraseLeaf),
    /// Satisfied by any of these index terms (possibly none).
    AnyOf(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ResolvedNode {
    Leaf(Leaf),
    And(Vec<ResolvedNode>),
    Or(Vec<ResolvedNode>),
    Not(Box<ResolvedNode>),
}

/// A query with every prefix and typo item expanded against a vocabulary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedQuery {
    pub(super) root: Option<ResolvedNode>,
    positive_terms: Vec<String>,
    estimated_bytes: usize,
}

impl CompiledQuery {
    /// Expands prefix and typo items against `vocabulary`.
    ///
    /// # Errors
    ///
    /// Returns a program-limit error when an item or the whole query expands
    /// past the declared limits, or the vocabulary's own error.
    pub fn resolve<V: Vocabulary + ?Sized>(
        &self,
        vocabulary: &V,
    ) -> Result<ResolvedQuery, ResolveError<V::Error>> {
        let Some(root) = &self.root else {
            return Ok(ResolvedQuery {
                root: None,
                positive_terms: Vec::new(),
                estimated_bytes: 0,
            });
        };
        let mut matchers = Vec::new();
        collect_matchers(root, &mut matchers);
        let expanded = expand(vocabulary, &matchers)?;
        let mut expanded = expanded.into_iter();
        let resolved = resolve_node(root, &mut expanded);
        let mut positive_terms = Vec::new();
        collect_positive_terms(&resolved, &mut positive_terms);
        positive_terms.sort();
        positive_terms.dedup();
        let estimated_bytes = node_bytes(root)
            .saturating_add(
                positive_terms
                    .iter()
                    .map(|term| term.len() + std::mem::size_of::<String>())
                    .sum::<usize>(),
            )
            .saturating_add(
                matchers
                    .len()
                    .saturating_mul(std::mem::size_of::<TermMatcher>()),
            );
        Ok(ResolvedQuery {
            root: Some(resolved),
            positive_terms,
            estimated_bytes,
        })
    }
}

impl ResolvedQuery {
    /// Whether the query can match nothing (blank or all stop words).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.root.is_none()
    }

    /// The sorted distinct index terms of positive items. Every matching
    /// document contains at least one of them, so their postings are the
    /// candidate set.
    #[must_use]
    pub fn positive_terms(&self) -> &[String] {
        &self.positive_terms
    }

    /// Approximate retained bytes, for query-memory accounting.
    #[must_use]
    pub fn estimated_bytes(&self) -> usize {
        self.estimated_bytes
    }
}

fn collect_matchers(node: &Node, matchers: &mut Vec<TermMatcher>) {
    match node {
        Node::Phrase(_) => {}
        Node::Prefix(word) => matchers.push(TermMatcher::Prefix(word.clone())),
        Node::Typo(word) => matchers.push(TermMatcher::OneEdit(word.clone())),
        Node::And(children) | Node::Or(children) => {
            for child in children {
                collect_matchers(child, matchers);
            }
        }
        Node::Not(child) => collect_matchers(child, matchers),
    }
}

/// Rebuilds the tree with expansions consumed in the order `collect_matchers`
/// produced them.
fn resolve_node(node: &Node, expanded: &mut std::vec::IntoIter<Vec<String>>) -> ResolvedNode {
    match node {
        Node::Phrase(phrase) => ResolvedNode::Leaf(Leaf::Phrase(phrase.clone())),
        Node::Prefix(_) | Node::Typo(_) => {
            ResolvedNode::Leaf(Leaf::AnyOf(expanded.next().unwrap_or_default()))
        }
        Node::And(children) => {
            ResolvedNode::And(children.iter().map(|c| resolve_node(c, expanded)).collect())
        }
        Node::Or(children) => {
            ResolvedNode::Or(children.iter().map(|c| resolve_node(c, expanded)).collect())
        }
        Node::Not(child) => ResolvedNode::Not(Box::new(resolve_node(child, expanded))),
    }
}

fn collect_positive_terms(node: &ResolvedNode, terms: &mut Vec<String>) {
    match node {
        ResolvedNode::Leaf(Leaf::Phrase(phrase)) => {
            terms.extend(phrase.terms.iter().map(|(_, term)| term.clone()));
        }
        ResolvedNode::Leaf(Leaf::AnyOf(any)) => terms.extend(any.iter().cloned()),
        ResolvedNode::And(children) | ResolvedNode::Or(children) => {
            for child in children {
                collect_positive_terms(child, terms);
            }
        }
        ResolvedNode::Not(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer::AnalyzerConfig;
    use crate::query::{SearchQueryInput, SearchSyntax};

    #[derive(Debug, PartialEq, Eq)]
    struct VocabularyFailure;

    struct FailingVocabulary;

    impl Vocabulary for FailingVocabulary {
        type Error = VocabularyFailure;

        fn matching_terms(
            &self,
            _matchers: &[TermMatcher],
            _limit: usize,
        ) -> Result<Vec<Vec<String>>, Self::Error> {
            Err(VocabularyFailure)
        }
    }

    #[test]
    fn should_preserve_original_vocabulary_error_given_resolution_failure() {
        // Arrange
        let input = SearchQueryInput::new("pre*", SearchSyntax::Query);
        let compiled =
            CompiledQuery::compile(&input, &AnalyzerConfig::default()).expect("query compiles");

        // Act
        let result = compiled.resolve(&FailingVocabulary);

        // Assert
        assert!(matches!(
            result,
            Err(ResolveError::Vocabulary(VocabularyFailure))
        ));
    }
}
