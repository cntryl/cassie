use crate::executor::batch::RowAccess;
use super::{Serialize, Deserialize, HashMap, AnalyzerConfig, HashSet, Value};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct SearchContext {
    pub(super) total_documents: usize,
    pub(super) doc_frequency: HashMap<String, HashMap<String, usize>>,
    pub(super) avg_doc_length: HashMap<String, f64>,
    doc_boost: HashMap<String, f64>,
    field_k1: HashMap<String, f64>,
    field_b: HashMap<String, f64>,
    field_analyzer: HashMap<String, AnalyzerConfig>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SearchTermStats {
    has_text: bool,
    doc_length: usize,
    term_counts: HashMap<String, usize>,
}

#[cfg(test)]
#[derive(Debug, Clone, Default)]
pub(crate) struct SingleFieldSearchContext {
    pub(super) total_documents: usize,
    doc_frequency: HashMap<String, usize>,
    avg_doc_length: f64,
    boost: f64,
    k1: f64,
    b: f64,
}

impl SearchTermStats {
    #[cfg(test)]
    pub(crate) fn from_text(source: Option<&str>) -> Self {
        Self::from_text_with_analyzer(source, &AnalyzerConfig::default())
    }

    pub(crate) fn from_text_with_analyzer(source: Option<&str>, analyzer: &AnalyzerConfig) -> Self {
        let Some(source) = source else {
            return Self::default();
        };
        let tokens = analyzer.analyze(source);
        Self {
            has_text: true,
            doc_length: tokens.len(),
            term_counts: token_counts(tokens.as_slice()),
        }
    }

    pub(crate) fn term_counts(&self) -> &HashMap<String, usize> {
        &self.term_counts
    }
}

#[cfg(test)]
impl SingleFieldSearchContext {
    pub(crate) fn from_term_stats<'a, I>(
        field: &str,
        documents: I,
        field_boost: &HashMap<String, f64>,
        field_k1: &HashMap<String, f64>,
        field_b: &HashMap<String, f64>,
    ) -> Self
    where
        I: IntoIterator<Item = &'a SearchTermStats>,
    {
        let field = field.to_ascii_lowercase();
        let mut context = Self {
            boost: field_boost.get(&field).copied().unwrap_or(1.0),
            k1: field_k1
                .get(&field)
                .copied()
                .unwrap_or(crate::search::bm25::DEFAULT_BM25_K1),
            b: field_b
                .get(&field)
                .copied()
                .unwrap_or(crate::search::bm25::DEFAULT_BM25_B),
            ..Default::default()
        };
        let mut docs_with_field = 0usize;
        let mut length_sum = 0usize;

        for stats in documents {
            context.total_documents += 1;
            if !stats.has_text {
                continue;
            }

            docs_with_field += 1;
            length_sum += stats.doc_length;
            for term in stats.term_counts.keys() {
                context
                    .doc_frequency
                    .entry(term.clone())
                    .and_modify(|count| *count += 1)
                    .or_insert(1);
            }
        }

        if docs_with_field > 0 {
            context.avg_doc_length = length_sum as f64 / docs_with_field as f64;
        }

        context
    }

    pub(crate) fn score_term_stats(
        &self,
        source_stats: &SearchTermStats,
        query_terms: &[String],
    ) -> f64 {
        if query_terms.is_empty() || !source_stats.has_text || source_stats.doc_length == 0 {
            return 0.0;
        }

        let dl = source_stats.doc_length as f64;
        let docs = self.total_documents.max(1) as f64;
        let avg_dl = if self.avg_doc_length > 0.0 {
            self.avg_doc_length
        } else {
            dl
        };

        let mut score = 0.0;
        for term in query_terms {
            let tf = source_stats.term_counts.get(term).copied().unwrap_or(0) as f64;
            if tf == 0.0 {
                continue;
            }

            let df = self.doc_frequency.get(term).copied().unwrap_or(0) as f64;
            score += crate::search::bm25_score(tf, df, docs, self.k1, self.b, dl, avg_dl);
        }

        score * self.boost
    }
}

impl SearchContext {
    pub(crate) fn from_rows<'a, I, R>(
        rows: I,
        text_fields: &[String],
        field_boost: &HashMap<String, f64>,
        field_k1: &HashMap<String, f64>,
        field_b: &HashMap<String, f64>,
        field_analyzer: &HashMap<String, AnalyzerConfig>,
    ) -> Self
    where
        I: IntoIterator<Item = &'a R>,
        R: RowAccess + 'a,
    {
        let mut context = Self {
            doc_boost: field_boost.clone(),
            field_k1: field_k1.clone(),
            field_b: field_b.clone(),
            field_analyzer: field_analyzer.clone(),
            ..Default::default()
        };

        let text_fields = text_fields
            .iter()
            .map(|field| field.to_lowercase())
            .collect::<HashSet<_>>();
        let mut term_occurrence = HashMap::<String, usize>::new();
        let mut text_length = HashMap::<String, usize>::new();

        for row in rows {
            context.total_documents += 1;
            for (name, value) in row.entries() {
                let name = name.to_lowercase();
                if !text_fields.is_empty() && !text_fields.contains(&name) {
                    continue;
                }

                let Value::String(text) = value else {
                    continue;
                };
                let analyzer = context.analyzer_for_field(&name);
                let term_stats = SearchTermStats::from_text_with_analyzer(Some(text), &analyzer);
                text_length
                    .entry(name.clone())
                    .and_modify(|value| *value += term_stats.doc_length)
                    .or_insert(term_stats.doc_length);
                *term_occurrence.entry(name.clone()).or_insert(0) += 1;
                for term in term_stats.term_counts.keys() {
                    context
                        .doc_frequency
                        .entry(name.clone())
                        .or_default()
                        .entry(term.clone())
                        .and_modify(|count| *count += 1)
                        .or_insert(1);
                }
            }
        }

        for (name, length_sum) in text_length {
            let docs_with_field = *term_occurrence.get(&name).unwrap_or(&1) as f64;
            if docs_with_field > 0.0 {
                context
                    .avg_doc_length
                    .insert(name, length_sum as f64 / docs_with_field);
            }
        }

        context
    }

    pub(crate) fn total_documents(&self) -> usize {
        self.total_documents
    }

    pub(crate) fn from_term_stats<'a, I>(
        field: &str,
        documents: I,
        field_boost: &HashMap<String, f64>,
        field_k1: &HashMap<String, f64>,
        field_b: &HashMap<String, f64>,
        field_analyzer: &HashMap<String, AnalyzerConfig>,
    ) -> Self
    where
        I: IntoIterator<Item = &'a SearchTermStats>,
    {
        let field = field.to_lowercase();
        let mut context = Self {
            doc_boost: field_boost.clone(),
            field_k1: field_k1.clone(),
            field_b: field_b.clone(),
            field_analyzer: field_analyzer.clone(),
            ..Default::default()
        };
        let mut docs_with_field = 0usize;
        let mut length_sum = 0usize;

        for stats in documents {
            context.total_documents += 1;
            if !stats.has_text {
                continue;
            }

            docs_with_field += 1;
            length_sum += stats.doc_length;
            for term in stats.term_counts.keys() {
                context
                    .doc_frequency
                    .entry(field.clone())
                    .or_default()
                    .entry(term.clone())
                    .and_modify(|count| *count += 1)
                    .or_insert(1);
            }
        }

        if docs_with_field > 0 {
            context
                .avg_doc_length
                .insert(field, length_sum as f64 / docs_with_field as f64);
        }

        context
    }

    fn average_doc_length(&self, field: &str) -> Option<f64> {
        self.avg_doc_length.get(&field.to_lowercase()).copied()
    }

    fn document_frequency(&self, field: &str, term: &str) -> Option<usize> {
        self.doc_frequency
            .get(&field.to_lowercase())
            .and_then(|terms| terms.get(&term.to_lowercase()).copied())
    }

    fn field_boost(&self, field: &str) -> f64 {
        self.doc_boost
            .get(&field.to_lowercase())
            .copied()
            .unwrap_or(1.0)
    }

    fn field_k1(&self, field: &str) -> f64 {
        self.field_k1
            .get(&field.to_lowercase())
            .copied()
            .unwrap_or(crate::search::bm25::DEFAULT_BM25_K1)
    }

    fn field_b(&self, field: &str) -> f64 {
        self.field_b
            .get(&field.to_lowercase())
            .copied()
            .unwrap_or(crate::search::bm25::DEFAULT_BM25_B)
    }

    pub(crate) fn score_text(&self, field: Option<&str>, source: &str, query: &str) -> f64 {
        let analyzer = field
            .map(|field| self.analyzer_for_field(field))
            .unwrap_or_default();
        let query_terms = prepare_query_terms_with_analyzer(query, &analyzer);
        let source_stats = SearchTermStats::from_text_with_analyzer(Some(source), &analyzer);
        self.score_term_stats(field, &source_stats, &query_terms)
    }

    pub(crate) fn analyzer_for_field(&self, field: &str) -> AnalyzerConfig {
        self.field_analyzer
            .get(&field.to_lowercase())
            .cloned()
            .unwrap_or_default()
    }

    pub(crate) fn score_term_stats(
        &self,
        field: Option<&str>,
        source_stats: &SearchTermStats,
        query_terms: &[String],
    ) -> f64 {
        if query_terms.is_empty() || !source_stats.has_text || source_stats.doc_length == 0 {
            return 0.0;
        }
        let dl = source_stats.doc_length as f64;

        let docs = self.total_documents.max(1) as f64;
        let field = field.map(str::to_lowercase);
        let avg_dl = field
            .as_deref()
            .and_then(|field| self.average_doc_length(field))
            .unwrap_or(dl);
        let boost = field
            .as_deref()
            .map_or(1.0, |field| self.field_boost(field));
        let (k1, b) = field
            .as_deref()
            .map_or((
                crate::search::bm25::DEFAULT_BM25_K1,
                crate::search::bm25::DEFAULT_BM25_B,
            ), |field| (self.field_k1(field), self.field_b(field)));

        let mut score = 0.0;
        for term in query_terms {
            let tf = source_stats.term_counts.get(term).copied().unwrap_or(0) as f64;
            if tf == 0.0 {
                continue;
            }

            let df = field
                .as_deref()
                .and_then(|field| self.document_frequency(field, term))
                .unwrap_or(0) as f64;
            score += crate::search::bm25_score(tf, df, docs, k1, b, dl, avg_dl);
        }

        score * boost
    }
}

pub(super) fn simple_search_score(haystack: &str, query: &str) -> f64 {
    if query.trim().is_empty() {
        return 0.0;
    }

    let haystack_tokens = crate::search::tokenizer::tokenize(haystack)
        .into_iter()
        .collect::<HashSet<_>>();
    let query_tokens = crate::search::tokenizer::tokenize(query);

    if query_tokens.is_empty() {
        return 0.0;
    }

    let mut hits = 0f64;
    for token in &query_tokens {
        if haystack_tokens.contains(token.as_str()) {
            hits += 1.0;
        }
    }
    hits / (query_tokens.len() as f64)
}

pub(super) fn token_counts(tokens: &[String]) -> HashMap<String, usize> {
    let mut out = HashMap::new();
    for token in tokens {
        out.entry(token.clone())
            .and_modify(|count| *count += 1)
            .or_insert(1);
    }
    out
}

#[cfg(test)]
pub(crate) fn prepare_query_terms(query: &str) -> Vec<String> {
    prepare_query_terms_with_analyzer(query, &AnalyzerConfig::default())
}

pub(crate) fn prepare_query_terms_with_analyzer(
    query: &str,
    analyzer: &AnalyzerConfig,
) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut terms = Vec::new();
    for token in analyzer.analyze(query) {
        if seen.insert(token.clone()) {
            terms.push(token);
        }
    }
    terms
}
