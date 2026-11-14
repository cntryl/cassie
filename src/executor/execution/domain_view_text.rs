//! Rewrites cast target type names inside stored SQL text (#828).
//!
//! A view keeps its definition as SQL text, so renaming or moving a domain a
//! view casts to must rename those references too. Only the two places a cast
//! names its target are touched: `expr::type` and `CAST(expr AS type)`.
//! String literals, quoted identifiers and comments are skipped.

const fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$' || byte >= 0x80
}

/// The offset just past the quoted run opened at `start`; a doubled quote is
/// an escaped quote.
fn quoted_run_end(bytes: &[u8], start: usize, quote: u8) -> usize {
    let mut index = start + 1;
    while index < bytes.len() {
        if bytes[index] == quote {
            if bytes.get(index + 1) == Some(&quote) {
                index += 2;
                continue;
            }
            return index + 1;
        }
        index += 1;
    }
    bytes.len()
}

fn skip_whitespace(bytes: &[u8], mut index: usize) -> usize {
    while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
        index += 1;
    }
    index
}

/// The offset just past the (optionally qualified and quoted) type name that
/// starts at `start`; equal to `start` when no name starts there.
fn type_name_end(bytes: &[u8], start: usize) -> usize {
    let mut index = start;
    loop {
        let part_start = index;
        if bytes.get(index) == Some(&b'"') {
            index = quoted_run_end(bytes, index, b'"');
        } else {
            while bytes.get(index).copied().is_some_and(is_identifier_byte) {
                index += 1;
            }
        }
        if index == part_start {
            return if part_start == start {
                start
            } else {
                part_start - 1
            };
        }
        if bytes.get(index) == Some(&b'.') {
            index += 1;
        } else {
            return index;
        }
    }
}

/// Whether the parenthesis at `open` belongs to a `CAST(` call.
fn opens_cast(bytes: &[u8], open: usize) -> bool {
    let mut end = open;
    while end > 0 && bytes[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    end >= 4
        && bytes[end - 4..end].eq_ignore_ascii_case(b"cast")
        && (end == 4 || !is_identifier_byte(bytes[end - 5]))
}

/// Whether the keyword `AS` stands alone at `index`.
fn is_as_keyword(bytes: &[u8], index: usize) -> bool {
    bytes
        .get(index..index + 2)
        .is_some_and(|word| word.eq_ignore_ascii_case(b"as"))
        && (index == 0 || !is_identifier_byte(bytes[index - 1]))
        && bytes.get(index + 2).is_some_and(u8::is_ascii_whitespace)
}

/// Returns `sql` with every cast target type name that `replace` maps to a new
/// spelling replaced, or `None` when nothing was replaced.
pub(super) fn rewrite_cast_targets(
    sql: &str,
    replace: &dyn Fn(&str) -> Option<String>,
) -> Option<String> {
    let bytes = sql.as_bytes();
    let mut rewritten = String::with_capacity(sql.len());
    let mut copied = 0;
    let mut changed = false;
    // One entry per open parenthesis: whether it belongs to `CAST(`.
    let mut cast_parentheses = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        let target_start = match bytes[index] {
            b'\'' | b'"' => {
                index = quoted_run_end(bytes, index, bytes[index]);
                continue;
            }
            b'-' if bytes.get(index + 1) == Some(&b'-') => {
                while index < bytes.len() && bytes[index] != b'\n' {
                    index += 1;
                }
                continue;
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index = sql[index + 2..]
                    .find("*/")
                    .map_or(bytes.len(), |end| index + 2 + end + 2);
                continue;
            }
            b'(' => {
                cast_parentheses.push(opens_cast(bytes, index));
                index += 1;
                continue;
            }
            b')' => {
                cast_parentheses.pop();
                index += 1;
                continue;
            }
            b':' if bytes.get(index + 1) == Some(&b':') => skip_whitespace(bytes, index + 2),
            b'a' | b'A'
                if cast_parentheses.last() == Some(&true) && is_as_keyword(bytes, index) =>
            {
                skip_whitespace(bytes, index + 2)
            }
            _ => {
                index += 1;
                continue;
            }
        };
        let target_end = type_name_end(bytes, target_start);
        if let Some(replacement) = replace(&sql[target_start..target_end]) {
            rewritten.push_str(&sql[copied..target_start]);
            rewritten.push_str(&replacement);
            copied = target_end;
            changed = true;
        }
        index = target_end.max(index + 2);
    }
    if !changed {
        return None;
    }
    rewritten.push_str(&sql[copied..]);
    Some(rewritten)
}

#[cfg(test)]
mod tests {
    use super::rewrite_cast_targets;

    fn rename_m(name: &str) -> Option<String> {
        (name == "m" || name == "public.m" || name == "\"m\"").then(|| "public.m2".to_owned())
    }

    #[test]
    fn should_rename_a_double_colon_cast_target() {
        // Arrange
        let sql = "SELECT id, x::m + 0 AS y FROM u1";

        // Act
        let rewritten = rewrite_cast_targets(sql, &rename_m);

        // Assert
        assert_eq!(
            rewritten.as_deref(),
            Some("SELECT id, x::public.m2 + 0 AS y FROM u1")
        );
    }

    #[test]
    fn should_rename_a_cast_function_target() {
        // Arrange
        let sql = "SELECT CAST(x AS m) AS m FROM u1";

        // Act
        let rewritten = rewrite_cast_targets(sql, &rename_m);

        // Assert
        assert_eq!(
            rewritten.as_deref(),
            Some("SELECT CAST(x AS public.m2) AS m FROM u1")
        );
    }

    #[test]
    fn should_rename_a_qualified_cast_target_at_the_end_of_the_text() {
        // Arrange
        let sql = "SELECT 5::public.m";

        // Act
        let rewritten = rewrite_cast_targets(sql, &rename_m);

        // Assert
        assert_eq!(rewritten.as_deref(), Some("SELECT 5::public.m2"));
    }

    #[test]
    fn should_leave_names_outside_cast_targets_untouched() {
        // Arrange
        let sql = "SELECT '::m' AS m, (SELECT 1 AS m) FROM m";

        // Act
        let rewritten = rewrite_cast_targets(sql, &rename_m);

        // Assert
        assert_eq!(rewritten, None);
    }

    #[test]
    fn should_leave_other_cast_targets_untouched() {
        // Arrange
        let sql = "SELECT x::int, CAST(y AS text), z::m FROM t";

        // Act
        let rewritten = rewrite_cast_targets(sql, &rename_m);

        // Assert
        assert_eq!(
            rewritten.as_deref(),
            Some("SELECT x::int, CAST(y AS text), z::public.m2 FROM t")
        );
    }
}
