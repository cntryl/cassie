pub const DEFAULT_BM25_K1: f64 = 1.2;
pub const DEFAULT_BM25_B: f64 = 0.75;
pub const DEFAULT_FULLTEXT_BOOST: f64 = 1.0;

pub fn bm25_score(tf: f64, df: f64, n: f64, k1: f64, b: f64, dl: f64, avgdl: f64) -> f64 {
    let idf = ((n - df + 0.5) / (df + 0.5) + 1.0).ln();
    idf * ((tf * (k1 + 1.0)) / (tf + k1 * (1.0 - b + b * (dl / avgdl.max(1.0)))))
}

pub fn snippet(text: &str, terms: &[String]) -> String {
    let mut out = text.to_string();
    for term in terms {
        if term.trim().is_empty() {
            continue;
        }

        if !contains_case_insensitive(&out, term) {
            continue;
        }

        out = highlight_term_case_insensitive(&out, term);
    }

    out
}

fn contains_case_insensitive(haystack: &str, needle: &str) -> bool {
    haystack.to_lowercase().contains(&needle.to_lowercase())
}

fn highlight_term_case_insensitive(haystack: &str, term: &str) -> String {
    if term.is_empty() {
        return haystack.to_string();
    }

    let lower_haystack = haystack.to_lowercase();
    let lower_term = term.to_lowercase();
    let mut out = String::new();
    let mut cursor = 0usize;

    while let Some(offset) = lower_haystack[cursor..].find(&lower_term) {
        let start = cursor + offset;
        let end = start + term.len();
        out.push_str(&haystack[cursor..start]);
        out.push_str(&format!("<mark>{}</mark>", &haystack[start..end]));
        cursor = end;
    }
    out.push_str(&haystack[cursor..]);
    out
}
