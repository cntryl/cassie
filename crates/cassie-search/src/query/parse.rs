//! Parser for the `'query'` syntax. Produces an unanalyzed expression tree.

use super::invalid;
use super::limits::{MAX_PHRASE_SLOP, MAX_QUERY_BYTES, MAX_QUERY_DEPTH, MAX_QUERY_ITEMS};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Expr {
    Word(String),
    Phrase { text: String, slop: u32 },
    Prefix(String),
    Typo(String),
    And(Vec<Expr>),
    Or(Vec<Expr>),
    Not(Box<Expr>),
}

impl Expr {
    /// Whether satisfying this expression requires at least one positive item.
    pub(super) fn is_positive(&self) -> bool {
        match self {
            Self::Word(_) | Self::Phrase { .. } | Self::Prefix(_) | Self::Typo(_) => true,
            Self::Not(_) => false,
            Self::And(children) => children.iter().any(Self::is_positive),
            Self::Or(children) => children.iter().all(Self::is_positive),
        }
    }

    /// Rejects expressions whose matches could not be reached from the
    /// postings of their positive terms.
    pub(super) fn validate_positive(&self) -> Result<(), super::SearchQueryError> {
        match self {
            Self::Word(_) | Self::Phrase { .. } | Self::Prefix(_) | Self::Typo(_) => Ok(()),
            Self::Not(child) => child.validate_positive(),
            Self::And(children) => {
                if !children.iter().any(Self::is_positive) {
                    return Err(invalid(
                        "a full-text query needs at least one term that is not excluded",
                    ));
                }
                children.iter().try_for_each(Self::validate_positive)
            }
            Self::Or(children) => {
                if !children.iter().all(Self::is_positive) {
                    return Err(invalid(
                        "every OR alternative of a full-text query needs a term that is not excluded",
                    ));
                }
                children.iter().try_for_each(Self::validate_positive)
            }
        }
    }
}

/// Parses `text`. A blank query parses to `None`.
pub(super) fn parse(text: &str) -> Result<Option<Expr>, super::SearchQueryError> {
    if text.len() > MAX_QUERY_BYTES {
        return Err(invalid(format!(
            "full-text query exceeds {MAX_QUERY_BYTES} bytes"
        )));
    }
    if text.trim().is_empty() {
        return Ok(None);
    }
    let mut parser = Parser {
        chars: text.chars().collect(),
        position: 0,
        items: 0,
    };
    let expr = parser.parse_or(0)?;
    parser.skip_whitespace();
    if parser.peek().is_some() {
        return Err(invalid("unbalanced ')' in full-text query"));
    }
    expr.validate_positive()?;
    if !expr.is_positive() {
        return Err(invalid(
            "a full-text query needs at least one term that is not excluded",
        ));
    }
    Ok(Some(expr))
}

struct Parser {
    chars: Vec<char>,
    position: usize,
    items: usize,
}

fn is_word_end(character: char) -> bool {
    character.is_whitespace() || matches!(character, '(' | ')' | '"')
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.position).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<char> {
        self.chars.get(self.position + offset).copied()
    }

    fn skip_whitespace(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.position += 1;
        }
    }

    /// Whether the upcoming characters are the keyword `keyword` followed by a
    /// separator (whitespace, a parenthesis, or the end of the text).
    fn at_keyword(&self, keyword: &str) -> bool {
        let length = keyword.chars().count();
        keyword
            .chars()
            .enumerate()
            .all(|(index, expected)| self.peek_at(index) == Some(expected))
            && self
                .peek_at(length)
                .is_none_or(|next| next.is_whitespace() || matches!(next, '(' | ')'))
    }

    fn at_item_end(&mut self) -> bool {
        self.skip_whitespace();
        self.peek().is_none_or(|character| character == ')')
    }

    fn parse_or(&mut self, depth: usize) -> Result<Expr, super::SearchQueryError> {
        let mut alternatives = vec![self.parse_and(depth)?];
        loop {
            self.skip_whitespace();
            if !self.at_keyword("OR") {
                break;
            }
            self.position += 2;
            if self.at_item_end() {
                return Err(invalid("OR needs an item on its right in full-text query"));
            }
            alternatives.push(self.parse_and(depth)?);
        }
        Ok(if alternatives.len() == 1 {
            alternatives.remove(0)
        } else {
            Expr::Or(alternatives)
        })
    }

    fn parse_and(&mut self, depth: usize) -> Result<Expr, super::SearchQueryError> {
        let mut items = Vec::new();
        loop {
            self.skip_whitespace();
            if self.peek().is_none_or(|character| character == ')') || self.at_keyword("OR") {
                break;
            }
            if self.at_keyword("AND") {
                if items.is_empty() {
                    return Err(invalid("AND needs an item on its left in full-text query"));
                }
                self.position += 3;
                if self.at_item_end() || self.at_keyword("OR") || self.at_keyword("AND") {
                    return Err(invalid("AND needs an item on its right in full-text query"));
                }
                continue;
            }
            items.push(self.parse_unary(depth)?);
        }
        match items.len() {
            0 => Err(invalid("expected a search item in full-text query")),
            1 => Ok(items.remove(0)),
            _ => Ok(Expr::And(items)),
        }
    }

    fn parse_unary(&mut self, depth: usize) -> Result<Expr, super::SearchQueryError> {
        let attached_next = self.peek_at(1).is_some_and(|next| !next.is_whitespace());
        if self.peek() == Some('-') && attached_next {
            self.position += 1;
            return self.parse_negated(depth);
        }
        if self.at_keyword("NOT") {
            self.position += 3;
            self.skip_whitespace();
            if self.peek().is_none_or(|character| character == ')') {
                return Err(invalid("NOT needs an item in full-text query"));
            }
            return self.parse_negated(depth);
        }
        if self.peek() == Some('+') && attached_next {
            self.position += 1;
        }
        self.parse_primary(depth)
    }

    fn parse_negated(&mut self, depth: usize) -> Result<Expr, super::SearchQueryError> {
        if depth + 1 > MAX_QUERY_DEPTH {
            return Err(invalid(format!(
                "full-text query nests deeper than {MAX_QUERY_DEPTH} levels"
            )));
        }
        let nested = (self.peek() == Some('-')
            && self.peek_at(1).is_some_and(|next| !next.is_whitespace()))
            || self.at_keyword("NOT");
        if nested {
            return Err(invalid("a full-text query item cannot be negated twice"));
        }
        Ok(Expr::Not(Box::new(self.parse_primary(depth + 1)?)))
    }

    fn parse_primary(&mut self, depth: usize) -> Result<Expr, super::SearchQueryError> {
        self.skip_whitespace();
        match self.peek() {
            Some('(') => {
                if depth + 1 > MAX_QUERY_DEPTH {
                    return Err(invalid(format!(
                        "full-text query nests deeper than {MAX_QUERY_DEPTH} levels"
                    )));
                }
                self.position += 1;
                let inner = self.parse_or(depth + 1)?;
                self.skip_whitespace();
                if self.peek() != Some(')') {
                    return Err(invalid("unbalanced '(' in full-text query"));
                }
                self.position += 1;
                Ok(inner)
            }
            Some('"') => {
                self.count_item()?;
                self.parse_phrase()
            }
            Some(_) => {
                self.count_item()?;
                self.parse_word()
            }
            None => Err(invalid("expected a search item in full-text query")),
        }
    }

    fn count_item(&mut self) -> Result<(), super::SearchQueryError> {
        self.items += 1;
        if self.items > MAX_QUERY_ITEMS {
            return Err(invalid(format!(
                "full-text query has more than {MAX_QUERY_ITEMS} items"
            )));
        }
        Ok(())
    }

    fn parse_phrase(&mut self) -> Result<Expr, super::SearchQueryError> {
        self.position += 1;
        let mut text = String::new();
        loop {
            match self.peek() {
                None => return Err(invalid("unterminated quote in full-text query")),
                Some('\\') => {
                    let Some(escaped) = self.peek_at(1) else {
                        return Err(invalid("dangling escape in full-text query"));
                    };
                    text.push(escaped);
                    self.position += 2;
                }
                Some('"') => {
                    self.position += 1;
                    break;
                }
                Some(character) => {
                    text.push(character);
                    self.position += 1;
                }
            }
        }
        let mut slop = 0u32;
        if self.peek() == Some('~') {
            self.position += 1;
            let digits = self.read_digits();
            if digits.is_empty() {
                return Err(invalid("a phrase slop needs a number after '~'"));
            }
            slop = parse_slop(&digits)?;
        }
        Ok(Expr::Phrase { text, slop })
    }

    fn read_digits(&mut self) -> String {
        let mut digits = String::new();
        while let Some(digit) = self.peek().filter(char::is_ascii_digit) {
            digits.push(digit);
            self.position += 1;
        }
        digits
    }

    fn parse_word(&mut self) -> Result<Expr, super::SearchQueryError> {
        // Each character is kept with whether it was escaped.
        let mut word: Vec<(char, bool)> = Vec::new();
        while let Some(character) = self.peek() {
            if is_word_end(character) {
                break;
            }
            if character == '\\' {
                let Some(escaped) = self.peek_at(1) else {
                    return Err(invalid("dangling escape in full-text query"));
                };
                word.push((escaped, true));
                self.position += 2;
            } else {
                word.push((character, false));
                self.position += 1;
            }
        }
        classify_word(&word)
    }
}

fn parse_slop(digits: &str) -> Result<u32, super::SearchQueryError> {
    digits
        .parse::<u32>()
        .ok()
        .filter(|slop| *slop <= MAX_PHRASE_SLOP)
        .ok_or_else(|| invalid(format!("phrase slop must be at most {MAX_PHRASE_SLOP}")))
}

fn text_of(word: &[(char, bool)]) -> String {
    word.iter().map(|(character, _)| character).collect()
}

/// Splits an optional unescaped `*`, `~` or `~1` suffix off a word.
fn classify_word(word: &[(char, bool)]) -> Result<Expr, super::SearchQueryError> {
    if let Some(((last, false), head)) = word.split_last() {
        if *last == '*' {
            return Ok(Expr::Prefix(text_of(head)));
        }
        if *last == '~' {
            return Ok(Expr::Typo(text_of(head)));
        }
    }
    let digits_start = word
        .iter()
        .rposition(|(character, escaped)| *escaped || !character.is_ascii_digit())
        .map_or(0, |index| index + 1);
    if digits_start > 0 && digits_start < word.len() {
        if let Some(('~', false)) = word.get(digits_start - 1).copied() {
            let digits = text_of(&word[digits_start..]);
            if digits == "1" {
                return Ok(Expr::Typo(text_of(&word[..digits_start - 1])));
            }
            return Err(invalid(
                "only single-edit typo matching ('word~' or 'word~1') is supported",
            ));
        }
    }
    Ok(Expr::Word(text_of(word)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::SearchQueryError;

    fn word(text: &str) -> Expr {
        Expr::Word(text.to_string())
    }

    #[test]
    fn should_parse_adjacent_items_as_conjunction_given_plain_words() {
        // Arrange
        let query = "red shoes";

        // Act
        let parsed = parse(query).expect("valid query");

        // Assert
        assert_eq!(parsed, Some(Expr::And(vec![word("red"), word("shoes")])));
    }

    #[test]
    fn should_bind_conjunction_tighter_than_or_given_mixed_operators() {
        // Arrange
        let query = "a b OR c";

        // Act
        let parsed = parse(query).expect("valid query");

        // Assert
        assert_eq!(
            parsed,
            Some(Expr::Or(vec![
                Expr::And(vec![word("a"), word("b")]),
                word("c")
            ]))
        );
    }

    #[test]
    fn should_bind_negation_tighter_than_and_given_dash_and_not_keyword() {
        // Arrange
        let dash = "a -b";
        let keyword = "a NOT b";

        // Act
        let dash = parse(dash).expect("valid dash query");
        let keyword = parse(keyword).expect("valid keyword query");

        // Assert
        let expected = Some(Expr::And(vec![word("a"), Expr::Not(Box::new(word("b")))]));
        assert_eq!(dash, expected);
        assert_eq!(keyword, expected);
    }

    #[test]
    fn should_accept_plus_as_required_marker_with_lowercase_keywords_as_words() {
        // Arrange
        let query = "+a or not";

        // Act
        let parsed = parse(query).expect("valid query");

        // Assert
        assert_eq!(
            parsed,
            Some(Expr::And(vec![word("a"), word("or"), word("not")]))
        );
    }

    #[test]
    fn should_parse_phrase_prefix_with_typo_items() {
        // Arrange
        let query = "\"a b\"~2 pre* typo~ edit~1";

        // Act
        let parsed = parse(query).expect("valid query");

        // Assert
        assert_eq!(
            parsed,
            Some(Expr::And(vec![
                Expr::Phrase {
                    text: "a b".to_string(),
                    slop: 2
                },
                Expr::Prefix("pre".to_string()),
                Expr::Typo("typo".to_string()),
                Expr::Typo("edit".to_string()),
            ]))
        );
    }

    #[test]
    fn should_keep_escaped_operator_characters_literal() {
        // Arrange
        let query = "a\\* \\-b \"say \\\"hi\\\"\"";

        // Act
        let parsed = parse(query).expect("valid query");

        // Assert
        assert_eq!(
            parsed,
            Some(Expr::And(vec![
                word("a*"),
                word("-b"),
                Expr::Phrase {
                    text: "say \"hi\"".to_string(),
                    slop: 0
                },
            ]))
        );
    }

    #[test]
    fn should_group_with_parentheses_over_precedence() {
        // Arrange
        let query = "a (b OR c)";

        // Act
        let parsed = parse(query).expect("valid query");

        // Assert
        assert_eq!(
            parsed,
            Some(Expr::And(vec![
                word("a"),
                Expr::Or(vec![word("b"), word("c")])
            ]))
        );
    }

    #[test]
    fn should_parse_blank_query_as_empty() {
        // Arrange
        let query = "   ";

        // Act
        let parsed = parse(query).expect("blank query");

        // Assert
        assert_eq!(parsed, None);
    }

    #[test]
    fn should_reject_malformed_queries_with_invalid_parameter_errors() {
        // Arrange
        let long = "a".repeat(MAX_QUERY_BYTES + 1);
        let many = vec!["w"; MAX_QUERY_ITEMS + 1].join(" ");
        let deep = format!("{}a{}", "(".repeat(MAX_QUERY_DEPTH + 1), ")".repeat(9));
        let queries = [
            "\"unterminated",
            "(a",
            "a)",
            "()",
            "a OR",
            "OR a",
            "a AND",
            "AND a",
            "NOT",
            "a\\",
            "-a",
            "a OR -b",
            "-a -b",
            "NOT NOT a",
            "a --b",
            "word~2",
            "\"a b\"~9",
            "\"a b\"~",
            long.as_str(),
            many.as_str(),
            deep.as_str(),
        ];

        // Act
        let errors = queries.map(parse);

        // Assert
        for (query, result) in queries.iter().zip(errors) {
            assert!(
                matches!(result, Err(SearchQueryError::InvalidParameter(_))),
                "expected an invalid-parameter error for {query:?}"
            );
        }
    }
}
