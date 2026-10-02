use super::analyzer::AnalyzerConfig;

pub const DEFAULT_BM25_K1: f64 = 1.2;
pub const DEFAULT_BM25_B: f64 = 0.75;
pub const DEFAULT_FULLTEXT_BOOST: f64 = 1.0;

#[must_use]
pub fn bm25_score(tf: f64, df: f64, n: f64, k1: f64, b: f64, dl: f64, avgdl: f64) -> f64 {
    if tf <= 0.0 || df <= 0.0 || n <= 0.0 {
        return 0.0;
    }

    let df = df.min(n);
    let idf = ((n - df + 0.5) / (df + 0.5) + 1.0).ln().max(0.0);
    let denominator = tf + k1 * (1.0 - b + b * (dl / avgdl.max(1.0)));
    if denominator <= 0.0 {
        return 0.0;
    }

    (idf * ((tf * (k1 + 1.0)) / denominator)).max(0.0)
}

/// Wraps every occurrence of an analyzed query term in `<mark>` tags, matching
/// tokens with the same analyzer that indexing and `search()` use.
#[must_use]
pub fn snippet(text: &str, terms: &[String], analyzer: &AnalyzerConfig) -> String {
    let spans = analyzer.matching_spans(text, terms);
    if spans.is_empty() {
        return text.to_string();
    }

    let mut out = String::with_capacity(text.len() + spans.len() * 13);
    let mut cursor = 0usize;
    for (start, end) in spans {
        out.push_str(&text[cursor..start]);
        out.push_str("<mark>");
        out.push_str(&text[start..end]);
        out.push_str("</mark>");
        cursor = end;
    }
    out.push_str(&text[cursor..]);
    out
}
