//! SQL `LIKE` pattern matching.
//!
//! Semantics follow PostgreSQL `LIKE`: matching is case-sensitive, `%` matches
//! any sequence of zero or more characters, `_` matches exactly one character,
//! and a backslash escapes the next pattern character so `%`, `_`, or `\` can
//! be matched literally. A pattern that ends with an unpaired backslash is
//! rejected. The `ESCAPE` clause and `ILIKE` are not part of the supported
//! grammar.
//!
//! The matcher walks the pattern and value in place, without allocating or
//! compiling the pattern, so it is safe to call once per row.

const ESCAPE: char = '\\';

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LikePatternError;

impl std::fmt::Display for LikePatternError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("LIKE pattern must not end with escape character")
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Token {
    AnySequence,
    AnyChar,
    Literal(char),
}

/// Reads the next pattern token, returning it with the remaining pattern.
fn next_token(pattern: &str) -> Result<Option<(Token, &str)>, LikePatternError> {
    let mut chars = pattern.chars();
    let Some(first) = chars.next() else {
        return Ok(None);
    };
    let token = match first {
        '%' => Token::AnySequence,
        '_' => Token::AnyChar,
        ESCAPE => Token::Literal(chars.next().ok_or(LikePatternError)?),
        literal => Token::Literal(literal),
    };
    Ok(Some((token, chars.as_str())))
}

fn split_first_char(value: &str) -> Option<(char, &str)> {
    let mut chars = value.chars();
    chars.next().map(|first| (first, chars.as_str()))
}

/// Returns whether `value` matches the SQL `LIKE` `pattern`.
///
/// # Errors
///
/// Returns [`LikePatternError`] when the pattern ends with an unpaired escape
/// character.
pub(crate) fn like_matches(value: &str, pattern: &str) -> Result<bool, LikePatternError> {
    let mut remaining_pattern = pattern;
    let mut remaining_value = value;
    // Resume point after the most recent `%`: the pattern after it and the
    // value position that `%` has consumed up to.
    let mut backtrack: Option<(&str, &str)> = None;

    loop {
        let step = match next_token(remaining_pattern)? {
            Some((Token::AnySequence, rest)) => {
                remaining_pattern = rest;
                backtrack = Some((rest, remaining_value));
                continue;
            }
            Some((Token::AnyChar, rest)) => {
                split_first_char(remaining_value).map(|(_, value_rest)| (rest, value_rest))
            }
            Some((Token::Literal(expected), rest)) => split_first_char(remaining_value)
                .filter(|(actual, _)| *actual == expected)
                .map(|(_, value_rest)| (rest, value_rest)),
            None if remaining_value.is_empty() => return Ok(true),
            None => None,
        };

        if let Some((pattern_rest, value_rest)) = step {
            remaining_pattern = pattern_rest;
            remaining_value = value_rest;
            continue;
        }

        // Mismatch: let the latest `%` absorb one more character and retry.
        let Some((pattern_after_wildcard, absorbed)) = backtrack else {
            return Ok(false);
        };
        let Some((_, absorbed_rest)) = split_first_char(absorbed) else {
            return Ok(false);
        };
        backtrack = Some((pattern_after_wildcard, absorbed_rest));
        remaining_pattern = pattern_after_wildcard;
        remaining_value = absorbed_rest;
    }
}

#[cfg(test)]
mod tests {
    use super::{like_matches, LikePatternError};

    fn matches(value: &str, pattern: &str) -> bool {
        like_matches(value, pattern).expect("valid pattern")
    }

    #[test]
    fn should_match_percent_wildcards_in_any_position() {
        // Arrange
        let cases = [
            ("foobar", "foo%bar", true),
            ("foo-x-bar", "foo%bar", true),
            ("foobaz", "foo%bar", false),
            ("abc", "%a%c%", true),
            ("xaybzc", "%a%c%", true),
            ("acb", "%a%c", false),
            ("foobar", "foo%%bar", true),
            ("", "%", true),
            ("anything", "%", true),
            ("abc", "abc", true),
            ("abcd", "abc", false),
            ("mississippi", "%iss%ppi", true),
        ];

        // Act
        let results = cases
            .iter()
            .map(|(value, pattern, _)| matches(value, pattern))
            .collect::<Vec<_>>();

        // Assert
        let expected = cases.iter().map(|case| case.2).collect::<Vec<_>>();
        assert_eq!(results, expected, "cases: {cases:?}");
    }

    #[test]
    fn should_match_underscore_as_exactly_one_character() {
        // Arrange
        let cases = [
            ("abc", "a_c", true),
            ("ac", "a_c", false),
            ("abbc", "a_c", false),
            ("aéc", "a_c", true),
            ("abc", "___", true),
            ("ab", "_%_", true),
            ("a", "_%_", false),
        ];

        // Act
        let results = cases
            .iter()
            .map(|(value, pattern, _)| matches(value, pattern))
            .collect::<Vec<_>>();

        // Assert
        let expected = cases.iter().map(|case| case.2).collect::<Vec<_>>();
        assert_eq!(results, expected, "cases: {cases:?}");
    }

    #[test]
    fn should_match_escaped_wildcards_literally() {
        // Arrange
        let cases = [
            ("a%c", r"a\%c", true),
            ("abc", r"a\%c", false),
            ("a_c", r"a\_c", true),
            ("abc", r"a\_c", false),
            (r"a\c", r"a\\c", true),
            ("100%", r"%\%", true),
        ];

        // Act
        let results = cases
            .iter()
            .map(|(value, pattern, _)| matches(value, pattern))
            .collect::<Vec<_>>();

        // Assert
        let expected = cases.iter().map(|case| case.2).collect::<Vec<_>>();
        assert_eq!(results, expected, "cases: {cases:?}");
    }

    #[test]
    fn should_match_case_sensitively() {
        // Arrange
        let value = "Alpha";

        // Act
        let exact = matches(value, "A%");
        let folded = matches(value, "a%");

        // Assert
        assert!(exact);
        assert!(!folded);
    }

    #[test]
    fn should_reject_pattern_ending_with_escape() {
        // Arrange
        let pattern = r"abc\";

        // Act
        let result = like_matches("abc", pattern);

        // Assert
        assert_eq!(result, Err(LikePatternError));
    }
}
