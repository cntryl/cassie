#![allow(dead_code)]
//! Independent BM25 reference for the Cranfield evaluation (#1368).
//!
//! Restates the documented contract with no Cassie code: the analyzer lowercases, splits on
//! non-alphanumeric characters and drops English stop words, then optionally applies the Snowball
//! English stemmer (`stemming = english`; the same `rust-stemmers` algorithm the analyzer
//! documents). Document count, document frequency and average length cover the whole collection.

use std::collections::{BTreeMap, BTreeSet};

pub const K1: f64 = 1.2;
pub const B: f64 = 0.75;

/// The English stop words of the default analyzer, as documented in `docs/fulltext-query-contract.md`.
const STOP_WORDS: [&str; 29] = [
    "a", "an", "and", "are", "as", "at", "be", "but", "by", "for", "from", "if", "in", "into",
    "is", "it", "no", "not", "of", "on", "or", "that", "the", "this", "to", "with", "was", "would",
    "you",
];

fn count(value: usize) -> f64 {
    f64::from(u32::try_from(value).expect("count fits in u32"))
}

/// Default analysis: lowercase, split on every non-alphanumeric character, drop empties and stop
/// words.
pub fn analyze(text: &str) -> Vec<String> {
    analyze_with(text, false)
}

/// Default analysis, then Snowball English stemming of each kept token when `stem` is set.
pub fn analyze_with(text: &str, stem: bool) -> Vec<String> {
    let stemmer = stem.then(|| rust_stemmers::Stemmer::create(rust_stemmers::Algorithm::English));
    text.to_lowercase()
        .split(|character: char| !character.is_alphanumeric())
        .filter(|token| !token.is_empty() && !STOP_WORDS.contains(token))
        .map(|token| {
            stemmer.as_ref().map_or_else(
                || token.to_string(),
                |stemmer| stemmer.stem(token).into_owned(),
            )
        })
        .collect()
}

/// Corpus statistics and per-document term frequencies.
pub struct Reference {
    stem: bool,
    ids: Vec<String>,
    lengths: Vec<f64>,
    frequencies: Vec<BTreeMap<String, usize>>,
    document_frequency: BTreeMap<String, usize>,
    average_length: f64,
}

impl Reference {
    /// Builds statistics over `(id, indexed text)` pairs with the default analyzer.
    pub fn new(documents: &[(String, String)]) -> Self {
        Self::with_stemming(documents, false)
    }

    /// Builds statistics over `(id, indexed text)` pairs, stemming when `stem` is set.
    pub fn with_stemming(documents: &[(String, String)], stem: bool) -> Self {
        let mut ids = Vec::new();
        let mut lengths = Vec::new();
        let mut frequencies = Vec::new();
        let mut document_frequency: BTreeMap<String, usize> = BTreeMap::new();
        for (id, text) in documents {
            let tokens = analyze_with(text, stem);
            let mut terms: BTreeMap<String, usize> = BTreeMap::new();
            for token in &tokens {
                *terms.entry(token.clone()).or_insert(0) += 1;
            }
            for term in terms.keys() {
                *document_frequency.entry(term.clone()).or_insert(0) += 1;
            }
            ids.push(id.clone());
            lengths.push(count(tokens.len()));
            frequencies.push(terms);
        }
        let average_length = (lengths.iter().sum::<f64>() / count(ids.len())).max(1.0);
        Self {
            stem,
            ids,
            lengths,
            frequencies,
            document_frequency,
            average_length,
        }
    }

    /// Every matching document's score for `query`, summed over its distinct analyzed terms.
    pub fn scores(&self, query: &str) -> BTreeMap<String, f64> {
        let total = count(self.ids.len());
        let terms: BTreeSet<String> = analyze_with(query, self.stem).into_iter().collect();
        let mut scores = BTreeMap::new();
        for term in &terms {
            let Some(&df) = self.document_frequency.get(term) else {
                continue;
            };
            let df = count(df);
            let idf = ((total - df + 0.5) / (df + 0.5) + 1.0).ln();
            for (index, id) in self.ids.iter().enumerate() {
                let Some(&tf) = self.frequencies[index].get(term) else {
                    continue;
                };
                let tf = count(tf);
                let norm = tf + K1 * (1.0 - B + B * self.lengths[index] / self.average_length);
                *scores.entry(id.clone()).or_insert(0.0) += idf * tf * (K1 + 1.0) / norm;
            }
        }
        scores
    }

    /// The top `limit` by score descending then id ascending (the SQL `ORDER BY`).
    pub fn top(&self, query: &str, limit: usize) -> Vec<(String, f64)> {
        let mut ranked: Vec<(String, f64)> = self.scores(query).into_iter().collect();
        ranked.sort_by(|left, right| {
            right
                .1
                .total_cmp(&left.1)
                .then_with(|| left.0.cmp(&right.0))
        });
        ranked.truncate(limit);
        ranked
    }
}

/// Outcome of comparing one query's Cassie page with the reference page.
#[derive(Debug, Clone, PartialEq)]
pub struct Agreement {
    pub same_ids: bool,
    pub max_score_difference: f64,
    pub within_tolerance: bool,
}

impl Agreement {
    pub fn is_full(&self) -> bool {
        self.same_ids && self.within_tolerance
    }
}

/// Ids and order must be identical; scores are compared pairwise within `tolerance`.
pub fn compare(cassie: &[(String, f64)], reference: &[(String, f64)], tolerance: f64) -> Agreement {
    let same_ids = cassie.len() == reference.len()
        && cassie
            .iter()
            .zip(reference)
            .all(|(left, right)| left.0 == right.0);
    let max_score_difference = cassie
        .iter()
        .zip(reference)
        .map(|(left, right)| (left.1 - right.1).abs())
        .fold(0.0, f64::max);
    Agreement {
        same_ids,
        max_score_difference,
        within_tolerance: max_score_difference <= tolerance,
    }
}
