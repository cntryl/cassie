use super::SqlError;

/// Returns the expression for the selected one-element GROUPING SETS form.
pub(super) fn single_expression(raw: &str) -> Result<Option<&str>, SqlError> {
    let keyword_start = crate::sql::parser::lexical::separator_end(raw, 0);
    let Some(keyword_end) =
        crate::sql::parser::lexical::pattern_end(raw, keyword_start, "grouping sets")
    else {
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
    if elements.len() != 1 {
        return Err(unsupported_group_by());
    }
    let expression = crate::sql::parser::lexical::trim_separators(elements[0]);
    if expression.is_empty() {
        return Err(unsupported_group_by());
    }
    Ok(Some(expression))
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
