use std::collections::HashSet;

pub fn tokenize(input: &str) -> Vec<String> {
    let stop = stop_words();
    input
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .filter(|t| !stop.contains(*t))
        .map(|t| t.to_string())
        .collect()
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
