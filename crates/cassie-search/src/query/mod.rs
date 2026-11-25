//! The `'query'` full-text syntax: phrases, prefixes, bounded typo matching and
//! Boolean intent over analyzed terms.
//!
//! The contract is documented in `docs/fulltext-query-contract.md`. The same
//! compiled and resolved query is evaluated by the authoritative row evaluator
//! and by every indexed path, over per-document token positions.

mod compile;
mod eval;
mod expand;
pub mod limits;
mod parse;
mod resolve;

use std::fmt::{Display, Formatter};

pub use compile::CompiledQuery;
pub use eval::DocumentPositions;
pub use expand::{matching_terms_in, TermMatcher, Vocabulary};
pub use resolve::{ResolveError, ResolvedQuery};

/// A query compilation or expansion error, independent of the execution engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchQueryError {
    /// The query text or syntax argument is invalid.
    InvalidParameter(String),
    /// The query exceeded a declared resource limit.
    ResourceLimit(String),
}

impl Display for SearchQueryError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidParameter(message) => {
                write!(formatter, "invalid parameter value: {message}")
            }
            Self::ResourceLimit(message) => write!(formatter, "resource limit exceeded: {message}"),
        }
    }
}

impl std::error::Error for SearchQueryError {}

/// How the text argument of `search`, `search_score` and `snippet` is read.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum SearchSyntax {
    /// Any analyzed token matches (the two-argument behavior).
    #[default]
    Terms,
    /// The phrase, prefix, typo and Boolean grammar.
    Query,
}

impl SearchSyntax {
    /// Reads the third argument of a search function.
    ///
    /// # Errors
    ///
    /// Returns an invalid-parameter error for any value other than `terms` or
    /// `query`.
    pub fn from_argument(value: &str) -> Result<Self, SearchQueryError> {
        match value {
            "terms" => Ok(Self::Terms),
            "query" => Ok(Self::Query),
            other => Err(invalid(format!(
                "unrecognized full-text query syntax \"{other}\"; expected 'terms' or 'query'"
            ))),
        }
    }
}

/// A search query text together with the syntax that reads it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SearchQueryInput {
    pub text: String,
    pub syntax: SearchSyntax,
}

impl SearchQueryInput {
    #[must_use]
    pub fn new(text: impl Into<String>, syntax: SearchSyntax) -> Self {
        Self {
            text: text.into(),
            syntax,
        }
    }
}

pub(crate) fn invalid(message: impl Into<String>) -> SearchQueryError {
    SearchQueryError::InvalidParameter(message.into())
}

pub(crate) fn limit_exceeded(message: impl Into<String>) -> SearchQueryError {
    SearchQueryError::ResourceLimit(message.into())
}
