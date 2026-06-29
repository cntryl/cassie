use std::collections::HashSet;

#[must_use]
pub fn tokenize(input: &str) -> Vec<String> {
    crate::search::analyzer::AnalyzerConfig::default().analyze(input)
}

pub fn stop_words() -> &'static HashSet<&'static str> {
    use std::sync::OnceLock;
    static WORDS: OnceLock<HashSet<&'static str>> = OnceLock::new();

    WORDS.get_or_init(|| {
        let words = [
            "a", "an", "and", "are", "as", "at", "be", "but", "by", "for", "from", "if", "in",
            "into", "is", "it", "no", "not", "of", "on", "or", "that", "the", "this", "to", "with",
            "was", "would", "you",
        ];
        words.into_iter().collect()
    })
}
