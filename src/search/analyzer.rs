use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AnalyzerConfig {
    pub name: String,
    pub tokenizer: String,
    pub case_folding: bool,
    pub stop_words: String,
    pub stemming: String,
    pub accent_folding: bool,
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
        }
    }
}

impl AnalyzerConfig {
    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn from_index_options(
        options: &std::collections::BTreeMap<String, String>,
    ) -> Result<Self, String> {
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
            if config.stemming != "none" {
                return Err(format!("unsupported stemming option '{}'", value.trim()));
            }
        }
        if let Some(value) = options.get("accent_folding") {
            config.accent_folding = parse_bool_option("accent_folding", value)?;
        }

        Ok(config)
    }

    pub fn analyze(&self, input: &str) -> Vec<String> {
        let normalized = if self.case_folding {
            input.to_lowercase()
        } else {
            input.to_string()
        };
        let normalized = if self.accent_folding {
            fold_accents(&normalized)
        } else {
            normalized
        };
        let stop_words = crate::search::tokenizer::stop_words();
        tokenize_with(&normalized, &self.tokenizer)
            .filter(|token| !token.is_empty())
            .filter(|token| self.stop_words != "english" || !stop_words.contains(*token))
            .map(ToString::to_string)
            .collect()
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
        let mut folded = String::with_capacity(input.len());
        let mut origins: Vec<(usize, usize)> = Vec::with_capacity(input.len());
        // `str::to_lowercase` is char-wise except for the context-sensitive final
        // sigma, so consume its output per source character to match `analyze`.
        let lowered = if self.case_folding {
            input.to_lowercase()
        } else {
            String::new()
        };
        let mut lowered_chars = lowered.chars();
        for (start, character) in input.char_indices() {
            let end = start + character.len_utf8();
            let before = folded.len();
            if self.case_folding {
                let produced = if character == '\u{3a3}' {
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

        let mut spans: Vec<(usize, usize)> = Vec::new();
        let mut token_start: Option<usize> = None;
        let boundaries = folded
            .char_indices()
            .map(|(index, character)| (index, is_token_boundary(&self.tokenizer, character)))
            .chain(std::iter::once((folded.len(), true)));
        for (index, boundary) in boundaries {
            match (token_start, boundary) {
                (None, false) => token_start = Some(index),
                (Some(start), true) => {
                    token_start = None;
                    if terms.iter().any(|term| term == &folded[start..index]) {
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

    #[must_use]
    pub fn cache_key(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| self.name.clone())
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

fn is_token_boundary(tokenizer: &str, character: char) -> bool {
    match tokenizer {
        "whitespace" => character.is_whitespace(),
        _ => !character.is_alphanumeric(),
    }
}

fn tokenize_with<'a>(input: &'a str, tokenizer: &'a str) -> impl Iterator<Item = &'a str> + 'a {
    input.split(move |character: char| is_token_boundary(tokenizer, character))
}

fn fold_accents(input: &str) -> String {
    input
        .chars()
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
}
