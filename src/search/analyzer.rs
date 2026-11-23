use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AnalyzerConfig {
    pub name: String,
    pub tokenizer: String,
    pub case_folding: bool,
    pub stop_words: String,
    pub stemming: String,
    pub accent_folding: bool,
    /// Symmetric synonym groups as `analyzed term -> canonical term` (the
    /// lexicographically smallest stemmed member of the group).
    #[serde(default)]
    pub synonyms: BTreeMap<String, String>,
}

/// Synonym option limits, enforced before an index definition is published.
pub const MAX_SYNONYM_GROUPS: usize = 256;
pub const MAX_SYNONYM_GROUP_MEMBERS: usize = 16;
pub const MAX_SYNONYM_MEMBER_BYTES: usize = 64;
pub const MAX_SYNONYM_OPTION_BYTES: usize = 16 * 1024;

/// One analyzed token located in its source text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenOccurrence {
    pub position: u32,
    pub term: String,
    /// Byte range in the original, unfolded text.
    pub span: (usize, usize),
}

#[derive(Debug, Clone)]
pub struct TokenizedText {
    pub tokens: Vec<String>,
}

impl Default for AnalyzerConfig {
    fn default() -> Self {
        Self {
            name: "standard".to_string(),
            tokenizer: "standard".to_string(),
            case_folding: true,
            stop_words: "english".to_string(),
            stemming: "none".to_string(),
            accent_folding: false,
            synonyms: BTreeMap::new(),
        }
    }
}

impl AnalyzerConfig {
    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn from_index_options(options: &BTreeMap<String, String>) -> Result<Self, String> {
        let mut config = Self::default();
        if let Some(name) = options.get("analyzer") {
            config.name = normalize_option(name);
            match config.name.as_str() {
                "standard" => {
                    config.stop_words = "english".to_string();
                }
                "simple" => {
                    config.stop_words = "none".to_string();
                }
                _ => return Err(format!("unsupported fulltext analyzer '{}'", name.trim())),
            }
        }
        if let Some(value) = options.get("case_folding") {
            config.case_folding = parse_bool_option("case_folding", value)?;
        }
        if let Some(value) = options.get("tokenizer") {
            config.tokenizer = normalize_option(value);
            if !matches!(config.tokenizer.as_str(), "standard" | "whitespace") {
                return Err(format!("unsupported tokenizer '{}'", value.trim()));
            }
        }
        if let Some(value) = options.get("stop_words") {
            config.stop_words = normalize_option(value);
            if !matches!(config.stop_words.as_str(), "english" | "none") {
                return Err(format!("unsupported stop_words option '{}'", value.trim()));
            }
        }
        if let Some(value) = options.get("stemming") {
            config.stemming = normalize_option(value);
            if !matches!(config.stemming.as_str(), "none" | "english") {
                return Err(format!("unsupported stemming option '{}'", value.trim()));
            }
        }
        if let Some(value) = options.get("accent_folding") {
            config.accent_folding = parse_bool_option("accent_folding", value)?;
        }
        if let Some(value) = options.get("synonyms") {
            config.synonyms = config.parse_synonyms(value)?;
        }

        Ok(config)
    }

    #[must_use]
    pub fn analyze(&self, input: &str) -> Vec<String> {
        self.analyze_positioned(input)
            .into_iter()
            .map(|(_, term)| term)
            .collect()
    }

    /// Analyzes `input` into `(position, term)` pairs.
    ///
    /// The terms are exactly those of [`Self::analyze`]. A position is the
    /// token's index among every token the tokenizer produced, so stop words
    /// removed by the analyzer still occupy a position and phrase matching
    /// preserves the gap they leave.
    #[must_use]
    pub fn analyze_positioned(&self, input: &str) -> Vec<(u32, String)> {
        let normalized = self.fold_case(input);
        let normalized = if self.accent_folding {
            fold_accents(&normalized)
        } else {
            normalized
        };
        let stop_words = crate::search::tokenizer::stop_words();
        let stemmer = self.stemmer();
        tokenize_with(&normalized, &self.tokenizer)
            .filter(|token| !token.is_empty())
            .enumerate()
            .filter(|(_, token)| self.stop_words != "english" || !stop_words.contains(*token))
            .map(|(position, token)| {
                (
                    u32::try_from(position).unwrap_or(u32::MAX),
                    self.canonical_term(stem_token(stemmer.as_ref(), token)),
                )
            })
            .collect()
    }

    /// Applies only the analyzer's case and accent folding, as used for prefix
    /// and typo query words, which are compared with stored analyzed terms.
    #[must_use]
    pub fn fold_query_word(&self, input: &str) -> String {
        let normalized = self.fold_case(input);
        if self.accent_folding {
            fold_accents(&normalized)
        } else {
            normalized
        }
    }

    /// Whether `word` contains a character the tokenizer splits on.
    #[must_use]
    pub fn has_token_boundary(&self, word: &str) -> bool {
        let mut boundaries = TokenBoundaries::new(&self.tokenizer);
        word.chars()
            .any(|character| boundaries.is_boundary(character))
    }

    fn canonical_term(&self, token: String) -> String {
        self.synonyms.get(&token).cloned().unwrap_or(token)
    }

    /// Parses `a,b;c,d` symmetric equivalence groups. Members are analyzed through
    /// the pipeline up to stemming; the smallest analyzed member is canonical.
    fn parse_synonyms(&self, raw: &str) -> Result<BTreeMap<String, String>, String> {
        let raw = raw.trim().trim_matches('\'').trim_matches('"');
        if raw.len() > MAX_SYNONYM_OPTION_BYTES {
            return Err(format!(
                "synonyms option exceeds {MAX_SYNONYM_OPTION_BYTES} bytes"
            ));
        }
        if raw.contains("=>") {
            return Err(
                "unsupported synonym syntax '=>'; use symmetric 'a,b;c,d' groups".to_string(),
            );
        }
        let base = Self {
            synonyms: BTreeMap::new(),
            ..self.clone()
        };
        let mut map = BTreeMap::new();
        let mut group_count = 0usize;
        for group in raw.split(';').filter(|group| !group.trim().is_empty()) {
            group_count += 1;
            if group_count > MAX_SYNONYM_GROUPS {
                return Err(format!("synonyms exceed {MAX_SYNONYM_GROUPS} groups"));
            }
            let members = group.split(',').map(str::trim).collect::<Vec<_>>();
            if members.len() > MAX_SYNONYM_GROUP_MEMBERS {
                return Err(format!(
                    "synonym group exceeds {MAX_SYNONYM_GROUP_MEMBERS} members"
                ));
            }
            let mut analyzed = std::collections::BTreeSet::new();
            for member in members {
                if member.len() > MAX_SYNONYM_MEMBER_BYTES {
                    return Err(format!(
                        "synonym member exceeds {MAX_SYNONYM_MEMBER_BYTES} bytes"
                    ));
                }
                let mut tokens = base.analyze(member);
                if tokens.len() != 1 {
                    return Err(format!(
                        "synonym member '{member}' must analyze to exactly one term"
                    ));
                }
                analyzed.insert(tokens.remove(0));
            }
            if analyzed.len() < 2 {
                return Err(format!(
                    "synonym group '{}' needs at least two distinct members",
                    group.trim()
                ));
            }
            let canonical = analyzed.iter().next().cloned().unwrap_or_default();
            for term in analyzed {
                if map.insert(term.clone(), canonical.clone()).is_some() {
                    return Err(format!(
                        "synonym term '{term}' appears in more than one group"
                    ));
                }
            }
        }
        Ok(map)
    }

    fn stemmer(&self) -> Option<rust_stemmers::Stemmer> {
        (self.stemming == "english")
            .then(|| rust_stemmers::Stemmer::create(rust_stemmers::Algorithm::English))
    }

    /// Folds `input` as `analyze` does and maps every folded byte back to the
    /// source character range it came from.
    fn fold_with_origins(&self, input: &str) -> (String, Vec<(usize, usize)>) {
        let mut folded = String::with_capacity(input.len());
        let mut origins: Vec<(usize, usize)> = Vec::with_capacity(input.len());
        // `str::to_lowercase` is char-wise except for the context-sensitive final
        // sigma, so consume its output per source character to match `analyze`.
        let lowered = if self.case_folding {
            self.fold_case(input)
        } else {
            String::new()
        };
        let mut lowered_chars = lowered.chars();
        for (start, character) in input.char_indices() {
            let end = start + character.len_utf8();
            let before = folded.len();
            if self.case_folding {
                let produced = if character == '\u{130}' || character == '\u{3a3}' {
                    1
                } else {
                    character.to_lowercase().count()
                };
                folded.extend(lowered_chars.by_ref().take(produced));
            } else {
                folded.push(character);
            }
            if self.accent_folding {
                let tail = fold_accents(&folded[before..]);
                folded.truncate(before);
                folded.push_str(&tail);
            }
            origins.resize(folded.len(), (start, end));
        }

        (folded, origins)
    }

    /// Returns the byte ranges of `input` whose analyzed tokens equal one of `terms`.
    ///
    /// Tokens are produced exactly as `analyze` produces them (case folding, accent
    /// folding, tokenizer boundaries), then mapped back onto the original text so
    /// callers can highlight the occurrences that made a search match.
    #[must_use]
    pub fn matching_spans(&self, input: &str, terms: &[String]) -> Vec<(usize, usize)> {
        if terms.is_empty() {
            return Vec::new();
        }
        let (folded, origins) = self.fold_with_origins(input);
        let stemmer = self.stemmer();
        let mut spans: Vec<(usize, usize)> = Vec::new();
        let mut token_start: Option<usize> = None;
        let mut token_boundaries = TokenBoundaries::new(&self.tokenizer);
        let boundaries = folded
            .char_indices()
            .map(|(index, character)| (index, token_boundaries.is_boundary(character)))
            .chain(std::iter::once((folded.len(), true)));
        for (index, boundary) in boundaries {
            match (token_start, boundary) {
                (None, false) => token_start = Some(index),
                (Some(start), true) => {
                    token_start = None;
                    let token =
                        self.canonical_term(stem_token(stemmer.as_ref(), &folded[start..index]));
                    if terms.contains(&token) {
                        let span = (origins[start].0, origins[index - 1].1);
                        if spans.last().is_none_or(|last| span.0 >= last.1) {
                            spans.push(span);
                        }
                    }
                }
                _ => {}
            }
        }
        spans
    }

    /// Returns every analyzed token of `input` with its position (gaps for stop
    /// words preserved, as in [`Self::analyze_positioned`]) and the byte range of
    /// the original text it came from.
    #[must_use]
    pub fn token_occurrences(&self, input: &str) -> Vec<TokenOccurrence> {
        let (folded, origins) = self.fold_with_origins(input);
        let stop_words = crate::search::tokenizer::stop_words();
        let stemmer = self.stemmer();
        let mut occurrences = Vec::new();
        let mut position = 0u32;
        let mut token_start: Option<usize> = None;
        let mut token_boundaries = TokenBoundaries::new(&self.tokenizer);
        let boundaries = folded
            .char_indices()
            .map(|(index, character)| (index, token_boundaries.is_boundary(character)))
            .chain(std::iter::once((folded.len(), true)));
        for (index, boundary) in boundaries {
            match (token_start, boundary) {
                (None, false) => token_start = Some(index),
                (Some(start), true) => {
                    token_start = None;
                    let raw = &folded[start..index];
                    if self.stop_words != "english" || !stop_words.contains(raw) {
                        occurrences.push(TokenOccurrence {
                            position,
                            term: self.canonical_term(stem_token(stemmer.as_ref(), raw)),
                            span: (origins[start].0, origins[index - 1].1),
                        });
                    }
                    position = position.saturating_add(1);
                }
                _ => {}
            }
        }
        occurrences
    }

    #[must_use]
    pub fn cache_key(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| self.name.clone())
    }

    fn fold_case(&self, input: &str) -> String {
        if self.case_folding {
            input
                .chars()
                .map(|character| if character == '\u{130}' { 'i' } else { character })
                .collect::<String>()
                .to_lowercase()
        } else {
            input.to_string()
        }
    }
}

impl TokenizedText {
    #[must_use]
    pub fn analyze(input: &str) -> Self {
        Self {
            tokens: AnalyzerConfig::default().analyze(input),
        }
    }
}

fn stem_token(stemmer: Option<&rust_stemmers::Stemmer>, token: &str) -> String {
    stemmer.map_or_else(
        || token.to_string(),
        |stemmer| stemmer.stem(token).into_owned(),
    )
}

fn normalize_option(value: &str) -> String {
    value
        .trim()
        .trim_matches('\'')
        .trim_matches('"')
        .to_ascii_lowercase()
}

fn parse_bool_option(name: &str, value: &str) -> Result<bool, String> {
    match normalize_option(value).as_str() {
        "true" | "on" | "1" => Ok(true),
        "false" | "off" | "0" => Ok(false),
        _ => Err(format!("{name} expects a boolean value")),
    }
}

/// Returns true for the combining mark blocks (Unicode category M) that attach
/// to the preceding base character.
fn is_combining_mark(character: char) -> bool {
    matches!(
        character,
        '\u{300}'..='\u{36f}'
            | '\u{1ab0}'..='\u{1aff}'
            | '\u{1dc0}'..='\u{1dff}'
            | '\u{20d0}'..='\u{20ff}'
            | '\u{fe20}'..='\u{fe2f}'
    )
}

/// Decides token boundaries one character at a time. A combining mark stays
/// inside the token it follows, so a decomposed accent or the dot left by
/// lowercasing `U+0130` never splits a word; a mark with no token to attach to
/// is a boundary like any other punctuation.
struct TokenBoundaries<'a> {
    tokenizer: &'a str,
    in_token: bool,
}

impl<'a> TokenBoundaries<'a> {
    fn new(tokenizer: &'a str) -> Self {
        Self {
            tokenizer,
            in_token: false,
        }
    }

    fn is_boundary(&mut self, character: char) -> bool {
        let boundary = match self.tokenizer {
            "whitespace" => character.is_whitespace(),
            _ if is_combining_mark(character) => !self.in_token,
            _ => !character.is_alphanumeric(),
        };
        self.in_token = !boundary;
        boundary
    }
}

fn tokenize_with<'a>(input: &'a str, tokenizer: &'a str) -> impl Iterator<Item = &'a str> + 'a {
    let mut boundaries = TokenBoundaries::new(tokenizer);
    input.split(move |character: char| boundaries.is_boundary(character))
}

fn fold_accents(input: &str) -> String {
    input
        .chars()
        .filter(|character| !is_combining_mark(*character))
        .map(|character| match character {
            'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' | 'ā' | 'ă' | 'ą' => 'a',
            'Á' | 'À' | 'Â' | 'Ä' | 'Ã' | 'Å' | 'Ā' | 'Ă' | 'Ą' => 'A',
            'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => 'c',
            'Ç' | 'Ć' | 'Ĉ' | 'Ċ' | 'Č' => 'C',
            'é' | 'è' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => 'e',
            'É' | 'È' | 'Ê' | 'Ë' | 'Ē' | 'Ĕ' | 'Ė' | 'Ę' | 'Ě' => 'E',
            'í' | 'ì' | 'î' | 'ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' => 'i',
            'Í' | 'Ì' | 'Î' | 'Ï' | 'Ĩ' | 'Ī' | 'Ĭ' | 'Į' => 'I',
            'ñ' | 'ń' | 'ņ' | 'ň' => 'n',
            'Ñ' | 'Ń' | 'Ņ' | 'Ň' => 'N',
            'ó' | 'ò' | 'ô' | 'ö' | 'õ' | 'ō' | 'ŏ' | 'ő' => 'o',
            'Ó' | 'Ò' | 'Ô' | 'Ö' | 'Õ' | 'Ō' | 'Ŏ' | 'Ő' => 'O',
            'ú' | 'ù' | 'û' | 'ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => 'u',
            'Ú' | 'Ù' | 'Û' | 'Ü' | 'Ũ' | 'Ū' | 'Ŭ' | 'Ů' | 'Ű' | 'Ų' => 'U',
            'ý' | 'ÿ' | 'ŷ' => 'y',
            'Ý' | 'Ŷ' => 'Y',
            _ => character,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn should_tokenize_with_standard_boundaries() {
        // Arrange
        let config = AnalyzerConfig::default();

        // Act
        let tokens = config.analyze("Alpha-Beta gamma");

        // Assert
        assert_eq!(tokens, vec!["alpha", "beta", "gamma"]);
    }

    #[test]
    fn should_keep_combining_mark_inside_standard_token() {
        // Arrange
        let config = AnalyzerConfig::default();

        // Act
        let tokens = config.analyze("cafe\u{301} bar");

        // Assert
        assert_eq!(tokens, vec!["cafe\u{301}", "bar"]);
    }

    #[test]
    fn should_keep_dotted_capital_i_word_whole() {
        // Arrange
        let config = AnalyzerConfig::default();

        // Act
        let tokens = config.analyze("\u{130}stanbul");

        // Assert
        assert_eq!(tokens, vec!["istanbul"]);
    }

    #[test]
    fn should_match_dotted_capital_i_only_with_case_folding() {
        // Arrange
        let simple = AnalyzerConfig {
            name: "simple".to_string(),
            stop_words: "none".to_string(),
            ..AnalyzerConfig::default()
        };
        let case_sensitive = AnalyzerConfig {
            case_folding: false,
            ..simple.clone()
        };

        // Act
        let folded = simple.analyze("\u{130}stanbul");
        let folded_query = simple.analyze("istanbul");
        let case_sensitive_index = case_sensitive.analyze("\u{130}stanbul");
        let case_sensitive_query = case_sensitive.analyze("istanbul");

        // Assert
        assert_eq!(folded, folded_query);
        assert_ne!(case_sensitive_index, case_sensitive_query);
    }

    #[test]
    fn should_preserve_precomposed_and_decomposed_accents_without_accent_folding() {
        // Arrange
        let config = AnalyzerConfig {
            name: "simple".to_string(),
            stop_words: "none".to_string(),
            ..AnalyzerConfig::default()
        };

        // Act
        let precomposed = config.analyze("caf\u{e9}");
        let decomposed = config.analyze("cafe\u{301}");

        // Assert
        assert_ne!(precomposed, decomposed);
    }

    #[test]
    fn should_not_start_token_at_standalone_combining_mark() {
        // Arrange
        let config = AnalyzerConfig::default();

        // Act
        let tokens = config.analyze("alpha \u{301} beta");

        // Assert
        assert_eq!(tokens, vec!["alpha", "beta"]);
    }

    #[test]
    fn should_strip_combining_marks_given_accent_folding() {
        // Arrange
        let config = AnalyzerConfig {
            accent_folding: true,
            ..AnalyzerConfig::default()
        };

        // Act
        let tokens = config.analyze("cafe\u{301} \u{130}stanbul");

        // Assert
        assert_eq!(tokens, vec!["cafe", "istanbul"]);
    }

    #[test]
    fn should_highlight_whole_word_with_combining_mark() {
        // Arrange
        let config = AnalyzerConfig::default();
        let text = "cafe\u{301} bar";
        let terms = vec!["cafe\u{301}".to_string()];

        // Act
        let spans = config.matching_spans(text, &terms);

        // Assert
        assert_eq!(spans, vec![(0, "cafe\u{301}".len())]);
    }

    #[test]
    fn should_tokenize_with_whitespace_boundaries() {
        // Arrange
        let config = AnalyzerConfig {
            tokenizer: "whitespace".to_string(),
            stop_words: "none".to_string(),
            ..AnalyzerConfig::default()
        };

        // Act
        let tokens = config.analyze("Alpha-Beta gamma");

        // Assert
        assert_eq!(tokens, vec!["alpha-beta", "gamma"]);
    }

    #[test]
    fn should_reject_unknown_fulltext_analyzer() {
        // Arrange
        let options = BTreeMap::from([("analyzer".to_string(), "unsupported".to_string())]);

        // Act
        let result = AnalyzerConfig::from_index_options(&options);

        // Assert
        let error = result.expect_err("unknown analyzer should fail");
        assert_eq!(error, "unsupported fulltext analyzer 'unsupported'");
    }

    #[test]
    fn should_reject_unknown_fulltext_tokenizer() {
        // Arrange
        let options = BTreeMap::from([("tokenizer".to_string(), "unsupported".to_string())]);

        // Act
        let result = AnalyzerConfig::from_index_options(&options);

        // Assert
        let error = result.expect_err("unknown tokenizer should fail");
        assert_eq!(error, "unsupported tokenizer 'unsupported'");
    }

    #[test]
    fn should_match_final_sigma_spans_like_analyze() {
        // Arrange
        let config = AnalyzerConfig::default();
        let text = "\u{39f}\u{394}\u{3a5}\u{3a3}\u{3a3}\u{395}\u{3a5}\u{3a3} home";
        let terms = config.analyze(text);

        // Act
        let spans = config.matching_spans(text, &terms[..1]);

        // Assert
        assert_eq!(spans, vec![(0, text.len() - " home".len())]);
    }

    #[test]
    fn should_match_whitespace_tokens_without_splitting_punctuation() {
        // Arrange
        let config = AnalyzerConfig {
            tokenizer: "whitespace".to_string(),
            stop_words: "none".to_string(),
            ..AnalyzerConfig::default()
        };
        let terms = config.analyze("alpha-beta");

        // Act
        let spans = config.matching_spans("x Alpha-Beta alpha", &terms);

        // Assert
        assert_eq!(spans, vec![(2, 12)]);
    }

    fn english_stemming() -> AnalyzerConfig {
        AnalyzerConfig {
            stemming: "english".to_string(),
            ..AnalyzerConfig::default()
        }
    }

    #[test]
    fn should_accept_english_stemming_option() {
        // Arrange
        let options = BTreeMap::from([("stemming".to_string(), "'English'".to_string())]);

        // Act
        let config = AnalyzerConfig::from_index_options(&options).expect("english stemming");

        // Assert
        assert_eq!(config.stemming, "english");
    }

    #[test]
    fn should_reject_unsupported_stemming_language() {
        // Arrange
        let options = BTreeMap::from([("stemming".to_string(), "french".to_string())]);

        // Act
        let result = AnalyzerConfig::from_index_options(&options);

        // Assert
        assert_eq!(
            result.expect_err("french is unsupported"),
            "unsupported stemming option 'french'"
        );
    }

    #[test]
    fn should_stem_inflected_terms_after_stop_words() {
        // Arrange
        let config = english_stemming();

        // Act
        let tokens = config.analyze("The Running accounts");

        // Assert
        assert_eq!(tokens, vec!["run", "account"]);
    }

    #[test]
    fn should_preserve_default_token_stream_without_stemming() {
        // Arrange
        let config = AnalyzerConfig::default();

        // Act
        let tokens = config.analyze("The Running accounts");

        // Assert
        assert_eq!(tokens, vec!["running", "accounts"]);
    }

    #[test]
    fn should_stem_after_accent_folding() {
        // Arrange
        let config = AnalyzerConfig {
            accent_folding: true,
            ..english_stemming()
        };

        // Act
        let tokens = config.analyze("Caf\u{e9}s");

        // Assert
        assert_eq!(tokens, vec!["cafe"]);
    }

    #[test]
    fn should_highlight_original_inflected_spans_with_stemming() {
        // Arrange
        let config = english_stemming();
        let text = "caf\u{e9} Connections and connected";
        let terms = config.analyze("connect");

        // Act
        let spans = config.matching_spans(text, &terms);

        // Assert
        let found: Vec<&str> = spans.iter().map(|(a, b)| &text[*a..*b]).collect();
        assert_eq!(found, vec!["Connections", "connected"]);
    }

    fn with_synonyms(raw: &str) -> Result<AnalyzerConfig, String> {
        AnalyzerConfig::from_index_options(&BTreeMap::from([
            ("stemming".to_string(), "english".to_string()),
            ("synonyms".to_string(), raw.to_string()),
        ]))
    }

    #[test]
    fn should_index_synonym_group_under_smallest_stemmed_member() {
        // Arrange
        let config = with_synonyms("'Cars, auto; TV, television'").expect("valid synonyms");

        // Act
        let tokens = config.analyze("Auto cars television tv");

        // Assert
        assert_eq!(tokens, vec!["auto", "auto", "televis", "televis"]);
    }

    #[test]
    fn should_highlight_original_source_of_synonym_matches() {
        // Arrange
        let config = with_synonyms("car,auto").expect("valid synonyms");
        let text = "A car and an AUTO";
        let terms = config.analyze("auto");

        // Act
        let spans = config.matching_spans(text, &terms);

        // Assert
        let found: Vec<&str> = spans.iter().map(|(a, b)| &text[*a..*b]).collect();
        assert_eq!(found, vec!["car", "AUTO"]);
    }

    #[test]
    fn should_keep_default_token_stream_when_no_synonyms_are_set() {
        // Arrange
        let config = AnalyzerConfig::from_index_options(&BTreeMap::new()).expect("defaults");

        // Act
        let tokens = config.analyze("car auto");

        // Assert
        assert_eq!(tokens, vec!["car", "auto"]);
    }

    #[test]
    fn should_reject_directional_synonym_syntax() {
        // Arrange
        let raw = "car=>auto";

        // Act
        let error = with_synonyms(raw).expect_err("directional rules are unsupported");

        // Assert
        assert!(error.contains("=>"), "{error}");
    }

    #[test]
    fn should_reject_terms_shared_by_two_synonym_groups() {
        // Arrange
        let raw = "car,auto;auto,vehicle";

        // Act
        let error = with_synonyms(raw).expect_err("overlap is rejected");

        // Assert
        assert!(error.contains("more than one group"), "{error}");
    }

    #[test]
    fn should_reject_singleton_or_multi_token_synonym_members() {
        // Arrange
        let single = "car";

        // Act
        let singleton = with_synonyms(single).expect_err("one member is not a group");
        let multi = with_synonyms("car,motor vehicle").expect_err("members are single terms");

        // Assert
        assert!(singleton.contains("at least two"), "{singleton}");
        assert!(multi.contains("exactly one term"), "{multi}");
    }

    #[test]
    fn should_reject_synonyms_beyond_documented_limits() {
        // Arrange
        let long_member = "a".repeat(MAX_SYNONYM_MEMBER_BYTES + 1);
        let many_members = (0..=MAX_SYNONYM_GROUP_MEMBERS)
            .map(|index| format!("m{index}x"))
            .collect::<Vec<_>>()
            .join(",");
        let many_groups = (0..=MAX_SYNONYM_GROUPS)
            .map(|index| format!("g{index}a,g{index}b"))
            .collect::<Vec<_>>()
            .join(";");

        // Act
        let member = with_synonyms(&format!("x,{long_member}")).expect_err("member too long");
        let members = with_synonyms(&many_members).expect_err("too many members");
        let groups = with_synonyms(&many_groups).expect_err("too many groups");
        let bytes = with_synonyms(&"a,b;".repeat(MAX_SYNONYM_OPTION_BYTES)).expect_err("too big");

        // Assert
        assert!(member.contains("bytes"), "{member}");
        assert!(members.contains("members"), "{members}");
        assert!(groups.contains("groups"), "{groups}");
        assert!(bytes.contains("bytes"), "{bytes}");
    }

    #[test]
    fn should_keep_stop_word_gaps_in_positions_without_changing_terms() {
        // Arrange
        let analyzer = AnalyzerConfig::default();
        let text = "Alpha the beta, and  GAMMA";

        // Act
        let positioned = analyzer.analyze_positioned(text);
        let terms = analyzer.analyze(text);

        // Assert
        assert_eq!(
            positioned,
            vec![
                (0, "alpha".to_string()),
                (2, "beta".to_string()),
                (4, "gamma".to_string())
            ]
        );
        assert_eq!(
            terms,
            positioned
                .into_iter()
                .map(|(_, term)| term)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn should_report_token_occurrences_with_source_spans_plus_matching_positions() {
        // Arrange
        let analyzer = AnalyzerConfig::default();
        let text = "Red the Shoes";

        // Act
        let occurrences = analyzer.token_occurrences(text);

        // Assert
        let found = occurrences
            .iter()
            .map(|occurrence| {
                (
                    occurrence.position,
                    occurrence.term.as_str(),
                    &text[occurrence.span.0..occurrence.span.1],
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(found, vec![(0, "red", "Red"), (2, "shoes", "Shoes")]);
        let positions = analyzer
            .analyze_positioned(text)
            .into_iter()
            .map(|(position, _)| position)
            .collect::<Vec<_>>();
        assert_eq!(
            positions,
            occurrences.iter().map(|o| o.position).collect::<Vec<_>>()
        );
    }
}
