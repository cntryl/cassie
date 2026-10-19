//! Lexically safe rewriting of `$n` placeholders in SQL text, used by
//! SQL-level `EXECUTE` to bind its arguments.

use super::lexical;

/// The highest `$n` placeholder in `sql`, ignoring quoted text and comments.
#[must_use]
pub(crate) fn max_positional_parameter(sql: &str) -> usize {
    let mut highest = 0;
    rewrite(sql, |index| {
        highest = highest.max(index);
        None
    });
    highest
}

/// Replaces each `$n` placeholder with `replacement(n)`; a `None`
/// replacement leaves the placeholder as written.
#[must_use]
pub(crate) fn substitute_positional_parameters(
    sql: &str,
    replacement: impl FnMut(usize) -> Option<String>,
) -> String {
    rewrite(sql, replacement)
}

fn rewrite(sql: &str, mut replacement: impl FnMut(usize) -> Option<String>) -> String {
    let bytes = sql.as_bytes();
    let mut output = String::with_capacity(sql.len());
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if matches!(byte, b'\'' | b'"' | b'$') {
            if let Some(end) = lexical::quoted_end(sql, index) {
                output.push_str(&sql[index..end]);
                index = end;
                continue;
            }
        }
        if bytes.get(index..index + 2) == Some(b"--") || bytes.get(index..index + 2) == Some(b"/*")
        {
            let end = lexical::separator_end(sql, index).max(index + 1);
            output.push_str(&sql[index..end]);
            index = end;
            continue;
        }
        let placeholder = byte == b'$'
            && !sql[..index]
                .chars()
                .next_back()
                .is_some_and(lexical::identifier_character);
        if placeholder {
            let digits_end = sql[index + 1..]
                .find(|character: char| !character.is_ascii_digit())
                .map_or(sql.len(), |offset| index + 1 + offset);
            if let Ok(number) = sql[index + 1..digits_end].parse::<usize>() {
                match replacement(number) {
                    Some(text) => output.push_str(&text),
                    None => output.push_str(&sql[index..digits_end]),
                }
                index = digits_end;
                continue;
            }
        }
        let character = sql[index..].chars().next().unwrap_or_default();
        output.push(character);
        index += character.len_utf8().max(1);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::{max_positional_parameter, substitute_positional_parameters};

    #[test]
    fn should_substitute_placeholders_given_quoted_and_commented_text() {
        // Arrange
        let sql = "SELECT $1, '$2', \"$3\", $$ $4 $$, $tag$ $5 $tag$ -- $6\n /* $7 */ $2";

        // Act
        let rewritten =
            substitute_positional_parameters(sql, |index| Some(format!("(arg{index})")));

        // Assert
        assert_eq!(
            rewritten,
            "SELECT (arg1), '$2', \"$3\", $$ $4 $$, $tag$ $5 $tag$ -- $6\n /* $7 */ (arg2)"
        );
        assert_eq!(max_positional_parameter(sql), 2);
    }

    #[test]
    fn should_ignore_dollar_signs_inside_identifiers_when_counting_placeholders() {
        // Arrange
        let sql = "SELECT a$1 FROM t WHERE x = $3";

        // Act
        let highest = max_positional_parameter(sql);

        // Assert
        assert_eq!(highest, 3);
    }
}
