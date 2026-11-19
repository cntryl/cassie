use super::SqlError;

/// Return the grouping key and whether the grammar includes a grand total.
pub(super) fn grouping_key(raw: &str) -> Result<Option<(&str, bool)>, SqlError> {
    let keyword_start = crate::sql::parser::lexical::separator_end(raw, 0);
    let lexical = crate::sql::parser::lexical::pattern_end;
    let rollup = lexical(raw, keyword_start, "rollup").filter(|end| {
        let open = crate::sql::parser::lexical::separator_end(raw, *end);
        raw.as_bytes().get(open) == Some(&b'(')
    });
    let Some(keyword_end) = rollup.or_else(|| lexical(raw, keyword_start, "grouping sets")) else {
        return Ok(None);
    };
    let open = crate::sql::parser::lexical::separator_end(raw, keyword_end);
    let Some(b'(') = raw.as_bytes().get(open) else {
        return Err(unsupported_group_by());
    };
    let Some(close) = crate::sql::parser::lexical::matching_paren(raw, open) else {
        return Err(unsupported_group_by());
    };
    if !crate::sql::parser::lexical::trim_separators(&raw[close + 1..]).is_empty() {
        return Err(unsupported_group_by());
    }

    let elements = super::split_csv(&raw[open + 1..close]);
    let (expression, subtotal) = match elements.as_slice() {
        [key] => (
            crate::sql::parser::lexical::trim_separators(key),
            rollup.is_some(),
        ),
        [detail, total]
            if rollup.is_none() && super::super::clauses::strip_parentheses(total) == Some("") =>
        {
            (
                super::super::clauses::strip_parentheses(detail)
                    .ok_or_else(unsupported_group_by)?,
                true,
            )
        }
        _ => return Err(unsupported_group_by()),
    };
    if expression.is_empty()
        || contains_unsupported_syntax(expression)
        || (subtotal
            && !matches!(
                super::parse_expression(expression),
                Ok(super::Expr::Column(_))
            ))
    {
        return Err(unsupported_group_by());
    }
    Ok(Some((expression, subtotal)))
}

pub(super) fn contains_unsupported_syntax(raw: &str) -> bool {
    let without_comments = crate::sql::parser::lexical::without_comments(raw);
    let raw = without_comments.as_ref();
    let mut index = 0;
    while index < raw.len() {
        if let Some(end) = crate::sql::parser::lexical::quoted_end(raw, index) {
            index = end;
            continue;
        }
        if crate::sql::parser::lexical::pattern_end(raw, index, "grouping sets").is_some() {
            return true;
        }
        for keyword in ["rollup", "cube"] {
            if let Some(end) = crate::sql::parser::lexical::pattern_end(raw, index, keyword) {
                let open = crate::sql::parser::lexical::separator_end(raw, end);
                if raw.as_bytes().get(open) == Some(&b'(') {
                    return true;
                }
            }
        }
        index += raw[index..].chars().next().map_or(0, char::len_utf8);
    }
    false
}

fn unsupported_group_by() -> SqlError {
    SqlError::new("unsupported GROUP BY syntax".into())
}
