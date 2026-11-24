//! Rewrites cast target type names inside stored SQL text (#828).
//!
//! A view keeps its definition as SQL text, so renaming or moving a domain a
//! view casts to must rename those references too. Only the two places a cast
//! names its target are touched: `expr::type` and `CAST(expr AS type)`.
//! Names are found with the literal-aware token scan, so string literals of
//! every form, quoted identifiers and comments are skipped.

use super::sql_text_tokens::{tokenize, Kind, Token};

/// The index just past the (optionally qualified and quoted) type name whose
/// first token is `first`; equal to `first` when no name starts there.
fn type_name_end(sql: &str, tokens: &[Token], first: usize) -> usize {
    let mut end = first;
    while tokens
        .get(end)
        .is_some_and(|token| matches!(token.kind, Kind::Word | Kind::Quoted))
    {
        end += 1;
        if tokens
            .get(end)
            .is_some_and(|token| token.is_punct(sql, b'.'))
        {
            end += 1;
        } else {
            return end;
        }
    }
    // A dangling `.` is not part of the name.
    end.saturating_sub(1).max(first)
}

/// The index of the first token of a cast target name that the token at
/// `index` introduces: `::` or `AS` inside `CAST(`.
fn target_after(sql: &str, tokens: &[Token], index: usize, in_cast: bool) -> Option<usize> {
    let token = tokens[index];
    if token.is_punct(sql, b':')
        && tokens
            .get(index + 1)
            .is_some_and(|next| next.is_punct(sql, b':'))
        && tokens[index + 1].start == token.end
    {
        return Some(index + 2);
    }
    (in_cast && token.is_keyword(sql, "as")).then_some(index + 1)
}

/// Returns `sql` with every cast target type name that `replace` maps to a new
/// spelling replaced, or `None` when nothing was replaced. Names inside string
/// literals of every form, quoted identifiers and comments are never touched.
pub(super) fn rewrite_cast_targets(
    sql: &str,
    replace: &dyn Fn(&str) -> Option<String>,
) -> Option<String> {
    let tokens = tokenize(sql);
    let mut rewritten = String::with_capacity(sql.len());
    let mut copied = 0;
    let mut changed = false;
    // One entry per open parenthesis: whether it belongs to `CAST(`.
    let mut cast_parentheses = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        let token = tokens[index];
        if token.is_punct(sql, b'(') {
            let opens_cast = index > 0 && tokens[index - 1].is_keyword(sql, "cast");
            cast_parentheses.push(opens_cast);
        } else if token.is_punct(sql, b')') {
            cast_parentheses.pop();
        } else if let Some(first) =
            target_after(sql, &tokens, index, cast_parentheses.last() == Some(&true))
        {
            let end = type_name_end(sql, &tokens, first);
            if end > first {
                let (start, stop) = (tokens[first].start, tokens[end - 1].end);
                if let Some(replacement) = replace(&sql[start..stop]) {
                    rewritten.push_str(&sql[copied..start]);
                    rewritten.push_str(&replacement);
                    copied = stop;
                    changed = true;
                }
                index = end;
                continue;
            }
        }
        index += 1;
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
