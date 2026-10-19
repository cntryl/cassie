use super::SqlError;

/// Returns the expression for the selected one-element GROUPING SETS form.
pub(super) fn single_expression(raw: &str) -> Result<Option<&str>, SqlError> {
    let Some(keyword_end) = crate::sql::parser::lexical::pattern_end(raw, 0, "grouping sets")
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

fn unsupported_group_by() -> SqlError {
    SqlError::new("unsupported GROUP BY syntax".into())
}
