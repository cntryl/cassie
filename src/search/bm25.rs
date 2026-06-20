pub const DEFAULT_BM25_K1: f64 = 1.2;
pub const DEFAULT_BM25_B: f64 = 0.75;
pub const DEFAULT_FULLTEXT_BOOST: f64 = 1.0;

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

pub fn snippet(text: &str, terms: &[String]) -> String {
    let mut normalized_terms = terms
        .iter()
        .map(|term| term.trim().to_lowercase())
        .filter(|term| !term.is_empty())
        .collect::<Vec<_>>();
    normalized_terms.sort_by_key(|term| std::cmp::Reverse(term.len()));
    normalized_terms.dedup();
    if normalized_terms.is_empty() {
        return text.to_string();
    }

    let lower_text = text.to_lowercase();
    let mut out = String::new();
    let mut cursor = 0usize;
    while cursor < text.len() {
        let matched = normalized_terms
            .iter()
            .find(|term| lower_text[cursor..].starts_with(term.as_str()));

        if let Some(term) = matched {
            let end = cursor + term.len();
            out.push_str("<mark>");
            out.push_str(&text[cursor..end]);
            out.push_str("</mark>");
            cursor = end;
            continue;
        }

        let Some(ch) = text[cursor..].chars().next() else {
            break;
        };
        out.push(ch);
        cursor += ch.len_utf8();
    }

    out
}
