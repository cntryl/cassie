//! A search query prepared for execution: either the any-token `'terms'` list
//! or a resolved `'query'` expression. Shared by the authoritative row
//! evaluator and the indexed read paths so both interpret queries identically.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::search::{prepare_query_terms_with_analyzer, SearchContext, SearchTermStats};
use crate::app::CassieError;
use crate::search::analyzer::{AnalyzerConfig, TokenOccurrence};
use crate::search::query::{
    matching_terms_in, CompiledQuery, DocumentPositions, ResolvedQuery, SearchQueryInput,
    SearchSyntax, TermMatcher, Vocabulary,
};

const PREPARED_QUERY_CACHE_LIMIT: usize = 64;

type PreparedQueryKey = (String, String);

/// Resolved queries of one statement's search context, keyed by field and text.
#[derive(Debug, Clone, Default)]
pub(crate) struct PreparedQueryCache(Arc<Mutex<HashMap<PreparedQueryKey, Arc<ResolvedQuery>>>>);

#[derive(Debug, Clone)]
pub(crate) enum PreparedSearch {
    /// Analyzed distinct query tokens; a row matches when it holds any of them.
    Terms(Vec<String>),
    /// A resolved `'query'` expression.
    Query(ResolvedQuery),
}

impl PreparedSearch {
    /// Prepares `input`, expanding prefix and typo items against `vocabulary`.
    ///
    /// # Errors
    ///
    /// Returns an invalid-parameter error for malformed `'query'` text and a
    /// program-limit error when an expansion limit is exceeded.
    pub(crate) fn prepare(
        input: &SearchQueryInput,
        analyzer: &AnalyzerConfig,
        vocabulary: &dyn Vocabulary,
    ) -> Result<Self, CassieError> {
        match input.syntax {
            SearchSyntax::Terms => Ok(Self::Terms(prepare_query_terms_with_analyzer(
                &input.text,
                analyzer,
            ))),
            SearchSyntax::Query => Ok(Self::Query(
                CompiledQuery::compile(input, analyzer)?.resolve(vocabulary)?,
            )),
        }
    }

    /// The index terms whose postings contain every possible match.
    pub(crate) fn candidate_terms(&self) -> &[String] {
        match self {
            Self::Terms(terms) => terms,
            Self::Query(query) => query.positive_terms(),
        }
    }

    /// Whether per-document statistics must carry token positions.
    pub(crate) fn needs_positions(&self) -> bool {
        matches!(self, Self::Query(_))
    }

    /// Approximate retained bytes, for query-memory accounting.
    pub(crate) fn estimated_bytes(&self) -> usize {
        match self {
            Self::Terms(terms) => terms
                .iter()
                .map(|term| term.len() + std::mem::size_of::<String>())
                .sum(),
            Self::Query(query) => query.estimated_bytes(),
        }
    }

    /// The BM25 score of a matching document, or `None` when it does not match.
    pub(crate) fn evaluate(
        &self,
        context: &SearchContext,
        field: Option<&str>,
        stats: &SearchTermStats,
    ) -> Option<f64> {
        match self {
            Self::Terms(terms) => {
                let score = context.score_term_stats(field, stats, terms);
                (score > 0.0).then_some(score)
            }
            Self::Query(query) => {
                let terms = query.matched_terms(stats)?;
                Some(context.score_term_stats(field, stats, &terms))
            }
        }
    }

    /// Statistics of a persisted document in the form `evaluate` needs.
    pub(crate) fn persisted_stats(
        &self,
        stats: &crate::midge::adapter::fulltext_retrieval::PersistedFulltextDocumentStats,
    ) -> SearchTermStats {
        if self.needs_positions() {
            SearchTermStats::from_persisted_with_positions(stats)
        } else {
            SearchTermStats::from_persisted(stats.doc_length, &stats.term_counts)
        }
    }

    /// `source` with the tokens that satisfied the query wrapped in `<mark>`.
    pub(crate) fn snippet(&self, source: &str, analyzer: &AnalyzerConfig) -> String {
        match self {
            Self::Terms(terms) => crate::search::snippet(source, terms, analyzer),
            Self::Query(query) => query_snippet(query, source, analyzer),
        }
    }
}

/// `source` with the tokens that satisfied `query` wrapped in `<mark>`.
pub(crate) fn query_snippet(
    query: &ResolvedQuery,
    source: &str,
    analyzer: &AnalyzerConfig,
) -> String {
    let occurrences = analyzer.token_occurrences(source);
    let document = OccurrencePositions::new(&occurrences);
    let positions = query.highlight_positions(&document);
    let mut spans: Vec<(usize, usize)> = Vec::new();
    for occurrence in &occurrences {
        if positions.binary_search(&occurrence.position).is_ok()
            && spans.last().is_none_or(|last| occurrence.span.0 >= last.1)
        {
            spans.push(occurrence.span);
        }
    }
    crate::search::render_snippet(source, &spans)
}

struct OccurrencePositions(HashMap<String, Vec<u32>>);

impl OccurrencePositions {
    fn new(occurrences: &[TokenOccurrence]) -> Self {
        let mut positions = HashMap::<String, Vec<u32>>::new();
        for occurrence in occurrences {
            positions
                .entry(occurrence.term.clone())
                .or_default()
                .push(occurrence.position);
        }
        Self(positions)
    }
}

impl DocumentPositions for OccurrencePositions {
    fn positions(&self, term: &str) -> Option<&[u32]> {
        self.0.get(term).map(Vec::as_slice)
    }
}

/// The corpus vocabulary of one field in a search context.
pub(crate) struct ContextVocabulary<'a> {
    context: &'a SearchContext,
    field: &'a str,
}

impl Vocabulary for ContextVocabulary<'_> {
    fn matching_terms(
        &self,
        matchers: &[TermMatcher],
        limit: usize,
    ) -> Result<Vec<Vec<String>>, CassieError> {
        let key = crate::sql::ColumnIdentifierPath::reference_field_key(self.field);
        let terms = self
            .context
            .doc_frequency
            .get(&key)
            .into_iter()
            .flat_map(|terms| terms.keys().map(String::as_str));
        Ok(matching_terms_in(terms, matchers, limit))
    }
}

/// The terms of a single document, used when no corpus statistics exist.
struct DocumentVocabulary<'a>(&'a SearchTermStats);

impl Vocabulary for DocumentVocabulary<'_> {
    fn matching_terms(
        &self,
        matchers: &[TermMatcher],
        limit: usize,
    ) -> Result<Vec<Vec<String>>, CassieError> {
        Ok(matching_terms_in(
            self.0.term_counts().keys().map(String::as_str),
            matchers,
            limit,
        ))
    }
}

impl SearchContext {
    /// The corpus vocabulary of `field`.
    pub(crate) fn vocabulary<'a>(&'a self, field: &'a str) -> ContextVocabulary<'a> {
        ContextVocabulary {
            context: self,
            field,
        }
    }

    /// Resolves a `'query'` text once per statement for row-wise evaluation.
    ///
    /// Without corpus statistics the vocabulary is the document's own terms.
    pub(crate) fn resolved_query(
        context: Option<&Self>,
        field: Option<&str>,
        input: &SearchQueryInput,
        document: &SearchTermStats,
    ) -> Result<Arc<ResolvedQuery>, CassieError> {
        let analyzer = context
            .zip(field)
            .map_or_else(AnalyzerConfig::default, |(context, field)| {
                context.analyzer_for_field(field)
            });
        let (Some(context), Some(field)) = (context, field) else {
            return Self::resolve_uncached(input, &analyzer, &DocumentVocabulary(document));
        };
        let key = (
            crate::sql::ColumnIdentifierPath::reference_field_key(field),
            input.text.clone(),
        );
        if let Ok(cache) = context.prepared_queries.0.lock() {
            if let Some(found) = cache.get(&key) {
                return Ok(Arc::clone(found));
            }
        }
        let resolved = Self::resolve_uncached(input, &analyzer, &context.vocabulary(field))?;
        if let Ok(mut cache) = context.prepared_queries.0.lock() {
            if cache.len() >= PREPARED_QUERY_CACHE_LIMIT {
                cache.clear();
            }
            cache.insert(key, Arc::clone(&resolved));
        }
        Ok(resolved)
    }

    fn resolve_uncached(
        input: &SearchQueryInput,
        analyzer: &AnalyzerConfig,
        vocabulary: &dyn Vocabulary,
    ) -> Result<Arc<ResolvedQuery>, CassieError> {
        Ok(Arc::new(
            CompiledQuery::compile(input, analyzer)?.resolve(vocabulary)?,
        ))
    }
}
