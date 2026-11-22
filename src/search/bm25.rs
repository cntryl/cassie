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
    let length_norm = 1.0 - b + b * (dl / avgdl.max(1.0));
    // `tf * (k1 + 1) / (tf + k1 * length_norm)`; past k1 = 1 both sides are
    // divided by k1 so a huge k1 cannot overflow the numerator or denominator.
    let saturation = if k1 > 1.0 {
        let inverse = 1.0 / k1;
        let denominator = tf * inverse + length_norm;
        if denominator <= 0.0 {
            return 0.0;
        }
        tf * (1.0 + inverse) / denominator
    } else {
        let denominator = tf + k1 * length_norm;
        if denominator <= 0.0 {
            return 0.0;
        }
        (tf * (k1 + 1.0)) / denominator
    };

    // `max` turns NaN into 0 and `min` keeps an overflowing product finite.
    (idf * saturation).max(0.0).min(f64::MAX)
}

/// Wraps every occurrence of an analyzed query term in `<mark>` tags, matching
/// tokens with the same analyzer that indexing and `search()` use.
#[must_use]
pub fn snippet(text: &str, terms: &[String], analyzer: &AnalyzerConfig) -> String {
    render_snippet(text, &analyzer.matching_spans(text, terms))
}

/// Wraps each byte range of `text` in `<mark>` tags. Ranges are sorted,
/// non-overlapping and on character boundaries.
#[must_use]
pub fn render_snippet(text: &str, spans: &[(usize, usize)]) -> String {
    if spans.is_empty() {
        return text.to_string();
    }

    let mut out = String::with_capacity(text.len() + spans.len() * 13);
    let mut cursor = 0usize;
    for &(start, end) in spans {
        out.push_str(&text[cursor..start]);
        out.push_str("<mark>");
        out.push_str(&text[start..end]);
        out.push_str("</mark>");
        cursor = end;
    }
    out.push_str(&text[cursor..]);
    out
}

#[cfg(test)]
mod tests {
    use super::bm25_score;

    #[test]
    fn should_return_finite_score_for_the_largest_finite_k1() {
        // Arrange
        let (tf, df, n, k1, b, dl, avgdl) = (3.0, 1.0, 10.0, 1e308, 0.0, 5.0, 5.0);

        // Act
        let score = bm25_score(tf, df, n, k1, b, dl, avgdl);

        // Assert
        assert!(score.is_finite() && score > 0.0, "score was {score}");
    }

    #[test]
    fn should_return_positive_score_when_numerator_and_denominator_overflow() {
        // Arrange
        let (tf, df, n, k1, b, dl, avgdl) = (1e10, 1.0, 10.0, 1e308, 1.0, 5.0, 1.0);

        // Act
        let score = bm25_score(tf, df, n, k1, b, dl, avgdl);

        // Assert
        assert!(score.is_finite() && score > 0.0, "score was {score}");
    }

    #[test]
    fn should_keep_the_classic_score_for_ordinary_k1() {
        // Arrange
        let (tf, df, n, k1, b, dl, avgdl) = (2.0, 1.0, 10.0, 1.2, 0.75, 5.0, 5.0);
        let idf = ((n - df + 0.5) / (df + 0.5) + 1.0_f64).ln();
        let expected = idf * (tf * (k1 + 1.0)) / (tf + k1 * (1.0 - b + b * (dl / avgdl)));

        // Act
        let score = bm25_score(tf, df, n, k1, b, dl, avgdl);

        // Assert
        assert!((score - expected).abs() < 1e-9);
    }
}
