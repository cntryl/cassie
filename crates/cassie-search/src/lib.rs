//! Text analysis, lexical scoring, and in-memory postings.

pub mod analyzer;
pub mod bm25;
pub mod inverted_index;
pub mod query;
pub mod tokenizer;

pub use analyzer::TokenizedText;
pub use bm25::{bm25_score, render_snippet, snippet};
pub use tokenizer::tokenize;

#[cfg(test)]
mod benchmark_fixture_tests;
