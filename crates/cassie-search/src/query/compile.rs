//! Analysis of a parsed query: words and phrases become position-aware term
//! sequences, prefix and typo words are folded, stop-word-only items are
//! dropped.

use super::limits::{MAX_PHRASE_SPAN, MIN_PREFIX_CHARS, MIN_TYPO_CHARS};
use super::parse::{parse, Expr};
use super::{invalid, SearchQueryInput, SearchSyntax};
use crate::analyzer::AnalyzerConfig;

/// An ordered term sequence. `terms` pairs each analyzed term with its offset
/// from the first term; offsets keep the gaps left by removed stop words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PhraseLeaf {
    pub(super) terms: Vec<(u32, String)>,
    pub(super) slop: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Node {
    Phrase(PhraseLeaf),
    Prefix(String),
    Typo(String),
    And(Vec<Node>),
    Or(Vec<Node>),
    Not(Box<Node>),
}

impl Node {
    fn is_positive(&self) -> bool {
        match self {
            Self::Phrase(_) | Self::Prefix(_) | Self::Typo(_) | Self::Or(_) => true,
            Self::Not(_) => false,
            Self::And(children) => children.iter().any(Self::is_positive),
        }
    }
}

/// A query that parsed and was analyzed, before vocabulary expansion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledQuery {
    pub(super) root: Option<Node>,
}

impl CompiledQuery {
    /// Parses and analyzes `input` with `analyzer`.
    ///
    /// # Errors
    ///
    /// Returns an invalid-parameter error when the text is not valid `'query'`
    /// syntax or exceeds a structural limit. `input` must use the
    /// [`SearchSyntax::Query`] syntax.
    pub fn compile(
        input: &SearchQueryInput,
        analyzer: &AnalyzerConfig,
    ) -> Result<Self, super::SearchQueryError> {
        debug_assert_eq!(input.syntax, SearchSyntax::Query);
        let Some(expr) = parse(&input.text)? else {
            return Ok(Self { root: None });
        };
        Ok(Self {
            root: compile_expr(&expr, analyzer)?,
        })
    }

    /// Approximate retained bytes, for query-memory accounting.
    #[must_use]
    pub fn estimated_bytes(&self) -> usize {
        self.root.as_ref().map_or(0, node_bytes)
    }
}

pub(super) fn node_bytes(node: &Node) -> usize {
    let base = std::mem::size_of::<Node>();
    base + match node {
        Node::Phrase(phrase) => phrase
            .terms
            .iter()
            .map(|(_, term)| term.len() + std::mem::size_of::<(u32, String)>())
            .sum::<usize>(),
        Node::Prefix(word) | Node::Typo(word) => word.len(),
        Node::And(children) | Node::Or(children) => children.iter().map(node_bytes).sum(),
        Node::Not(child) => node_bytes(child),
    }
}

fn compile_expr(
    expr: &Expr,
    analyzer: &AnalyzerConfig,
) -> Result<Option<Node>, super::SearchQueryError> {
    match expr {
        Expr::Word(text) => compile_phrase(text, 0, analyzer),
        Expr::Phrase { text, slop } => compile_phrase(text, *slop, analyzer),
        Expr::Prefix(word) => fold_single_token(word, analyzer, MIN_PREFIX_CHARS, "prefix")
            .map(|word| Some(Node::Prefix(word))),
        Expr::Typo(word) => fold_single_token(word, analyzer, MIN_TYPO_CHARS, "typo")
            .map(|word| Some(Node::Typo(word))),
        Expr::And(children) => {
            let nodes = compile_children(children, analyzer)?;
            Ok(match nodes.len() {
                0 => None,
                _ if !nodes.iter().any(Node::is_positive) => None,
                1 => nodes.into_iter().next(),
                _ => Some(Node::And(nodes)),
            })
        }
        Expr::Or(children) => {
            let nodes = compile_children(children, analyzer)?;
            Ok(match nodes.len() {
                0 => None,
                1 => nodes.into_iter().next(),
                _ => Some(Node::Or(nodes)),
            })
        }
        Expr::Not(child) => {
            Ok(compile_expr(child, analyzer)?.map(|node| Node::Not(Box::new(node))))
        }
    }
}

fn compile_children(
    children: &[Expr],
    analyzer: &AnalyzerConfig,
) -> Result<Vec<Node>, super::SearchQueryError> {
    let mut nodes = Vec::with_capacity(children.len());
    for child in children {
        if let Some(node) = compile_expr(child, analyzer)? {
            nodes.push(node);
        }
    }
    Ok(nodes)
}

fn compile_phrase(
    text: &str,
    slop: u32,
    analyzer: &AnalyzerConfig,
) -> Result<Option<Node>, super::SearchQueryError> {
    let tokens = analyzer.analyze_positioned(text);
    let (Some((first, _)), Some((last, _))) = (tokens.first(), tokens.last()) else {
        return Ok(None);
    };
    let (first, last) = (*first, *last);
    let span = usize::try_from(last - first)
        .unwrap_or(usize::MAX)
        .saturating_add(1);
    if span > MAX_PHRASE_SPAN {
        return Err(invalid(format!(
            "a full-text phrase spans {span} token positions; the limit is {MAX_PHRASE_SPAN}"
        )));
    }
    let terms = tokens
        .into_iter()
        .map(|(position, term)| (position - first, term))
        .collect();
    Ok(Some(Node::Phrase(PhraseLeaf { terms, slop })))
}

fn fold_single_token(
    word: &str,
    analyzer: &AnalyzerConfig,
    minimum: usize,
    kind: &str,
) -> Result<String, super::SearchQueryError> {
    let folded = analyzer.fold_query_word(word);
    if analyzer.has_token_boundary(&folded) {
        return Err(invalid(format!(
            "a full-text {kind} word must be a single token: '{word}'"
        )));
    }
    if folded.chars().count() < minimum {
        return Err(invalid(format!(
            "a full-text {kind} word needs at least {minimum} characters: '{word}'"
        )));
    }
    Ok(folded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::SearchQueryError;

    fn compile(text: &str) -> Result<CompiledQuery, SearchQueryError> {
        CompiledQuery::compile(
            &SearchQueryInput::new(text, SearchSyntax::Query),
            &AnalyzerConfig::default(),
        )
    }

    fn phrase(terms: &[(u32, &str)], slop: u32) -> Node {
        Node::Phrase(PhraseLeaf {
            terms: terms
                .iter()
                .map(|(offset, term)| (*offset, (*term).to_string()))
                .collect(),
            slop,
        })
    }

    #[test]
    fn should_preserve_stop_word_gaps_in_phrases() {
        // Arrange
        let query = "\"alpha the beta\"";

        // Act
        let compiled = compile(query).expect("valid query");

        // Assert
        assert_eq!(compiled.root, Some(phrase(&[(0, "alpha"), (2, "beta")], 0)));
    }

    #[test]
    fn should_trim_leading_with_trailing_stop_words_from_phrases() {
        // Arrange
        let query = "\"the alpha beta of\"";

        // Act
        let compiled = compile(query).expect("valid query");

        // Assert
        assert_eq!(compiled.root, Some(phrase(&[(0, "alpha"), (1, "beta")], 0)));
    }

    #[test]
    fn should_treat_multi_token_word_as_implicit_phrase() {
        // Arrange
        let query = "alpha-beta";

        // Act
        let compiled = compile(query).expect("valid query");

        // Assert
        assert_eq!(compiled.root, Some(phrase(&[(0, "alpha"), (1, "beta")], 0)));
    }

    #[test]
    fn should_drop_stop_word_items_keeping_remaining_conjunction() {
        // Arrange
        let query = "the cat";

        // Act
        let compiled = compile(query).expect("valid query");

        // Assert
        assert_eq!(compiled.root, Some(phrase(&[(0, "cat")], 0)));
    }

    #[test]
    fn should_match_nothing_given_only_stop_words_or_an_empty_query() {
        // Arrange
        let queries = ["the", "the -cat", "   ", "\"of the\""];

        // Act
        let roots = queries.map(|query| compile(query).expect("valid query").root);

        // Assert
        assert!(roots.iter().all(Option::is_none));
    }

    #[test]
    fn should_fold_prefix_with_typo_words_without_stemming() {
        // Arrange
        let analyzer = AnalyzerConfig {
            stemming: "english".to_string(),
            accent_folding: true,
            ..AnalyzerConfig::default()
        };
        let input = SearchQueryInput::new("Runn* Café~", SearchSyntax::Query);

        // Act
        let compiled = CompiledQuery::compile(&input, &analyzer).expect("valid query");

        // Assert
        assert_eq!(
            compiled.root,
            Some(Node::And(vec![
                Node::Prefix("runn".to_string()),
                Node::Typo("cafe".to_string())
            ]))
        );
    }

    #[test]
    fn should_reject_short_prefix_short_typo_with_multi_token_prefix() {
        // Arrange
        let queries = ["a*", "abc~", "foo-bar*"];

        // Act
        let results = queries.map(compile);

        // Assert
        assert!(results
            .iter()
            .all(|result| matches!(result, Err(SearchQueryError::InvalidParameter(_)))));
    }

    #[test]
    fn should_reject_phrase_longer_than_the_span_limit() {
        // Arrange
        let query = format!("\"{}\"", vec!["word"; MAX_PHRASE_SPAN + 1].join(" "));

        // Act
        let result = compile(&query);

        // Assert
        assert!(matches!(result, Err(SearchQueryError::InvalidParameter(_))));
    }
}
