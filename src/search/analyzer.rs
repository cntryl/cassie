use crate::search::tokenizer;

#[derive(Debug, Clone)]
pub struct TokenizedText {
    pub tokens: Vec<String>,
}

impl TokenizedText {
    pub fn analyze(input: &str) -> Self {
        Self {
            tokens: tokenizer::tokenize(input),
        }
    }
}
