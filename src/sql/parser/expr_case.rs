use super::super::{Expr, SqlError};
use super::parse_expression;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Keyword {
    When,
    Then,
    Else,
    End,
}

fn case_keywords(raw: &str) -> Result<Vec<(usize, Keyword)>, SqlError> {
    let bytes = raw.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;
    let mut parentheses = 0u32;
    let mut cases = 0u32;
    let mut single_quote = false;
    let mut double_quote = false;
    while index < bytes.len() {
        match bytes[index] {
            b'\'' if !double_quote => single_quote = !single_quote,
            b'"' if !single_quote => double_quote = !double_quote,
            b'(' if !single_quote && !double_quote => parentheses += 1,
            b')' if !single_quote && !double_quote => {
                parentheses = parentheses.saturating_sub(1);
            }
            byte if !single_quote
                && !double_quote
                && (byte.is_ascii_alphabetic() || byte == b'_') =>
            {
                let start = index;
                while index < bytes.len()
                    && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
                {
                    index += 1;
                }
                let word = &raw[start..index];
                if word.eq_ignore_ascii_case("case") {
                    cases += 1;
                } else if word.eq_ignore_ascii_case("end") {
                    if cases == 0 {
                        return Err(SqlError::new("CASE has an unexpected END".into()));
                    }
                    cases -= 1;
                    if cases == 0 {
                        tokens.push((start, Keyword::End));
                    }
                } else if cases == 1 && parentheses == 0 {
                    let keyword = if word.eq_ignore_ascii_case("when") {
                        Some(Keyword::When)
                    } else if word.eq_ignore_ascii_case("then") {
                        Some(Keyword::Then)
                    } else if word.eq_ignore_ascii_case("else") {
                        Some(Keyword::Else)
                    } else {
                        None
                    };
                    if let Some(keyword) = keyword {
                        tokens.push((start, keyword));
                    }
                }
                continue;
            }
            _ => {}
        }
        index += 1;
    }
    if cases != 0 || single_quote || double_quote {
        return Err(SqlError::new("CASE requires a matching END".into()));
    }
    Ok(tokens)
}

fn segment<'a>(raw: &'a str, start: usize, end: usize, context: &str) -> Result<&'a str, SqlError> {
    let value = raw[start..end].trim();
    if value.is_empty() {
        Err(SqlError::new(format!("CASE requires {context}")))
    } else {
        Ok(value)
    }
}

pub(super) fn parse_case(raw: &str) -> Result<Expr, SqlError> {
    let raw = raw.trim();
    let tokens = case_keywords(raw)?;
    let Some((first_when, Keyword::When)) = tokens.first().copied() else {
        return Err(SqlError::new("CASE requires WHEN and THEN".into()));
    };
    let operand = if raw[4..first_when].trim().is_empty() {
        None
    } else {
        Some(Box::new(parse_expression(segment(
            raw,
            4,
            first_when,
            "an operand",
        )?)?))
    };
    let mut branches = Vec::new();
    let mut else_expr = None;
    let mut cursor = 0;
    while cursor < tokens.len() {
        let (when_start, kind) = tokens[cursor];
        if kind != Keyword::When {
            return Err(SqlError::new("CASE requires WHEN before THEN".into()));
        }
        let Some(&(then_start, Keyword::Then)) = tokens.get(cursor + 1) else {
            return Err(SqlError::new("CASE WHEN requires THEN".into()));
        };
        let condition = parse_expression(segment(
            raw,
            when_start + 4,
            then_start,
            "a WHEN expression",
        )?)?;
        let Some(&(next_start, next_kind)) = tokens.get(cursor + 2) else {
            return Err(SqlError::new("CASE requires END".into()));
        };
        let result = parse_expression(segment(
            raw,
            then_start + 4,
            next_start,
            "a THEN expression",
        )?)?;
        branches.push((condition, result));
        match next_kind {
            Keyword::When => cursor += 2,
            Keyword::Else => {
                let Some(&(end_start, Keyword::End)) = tokens.get(cursor + 3) else {
                    return Err(SqlError::new("CASE ELSE requires END".into()));
                };
                else_expr = Some(Box::new(parse_expression(segment(
                    raw,
                    next_start + 4,
                    end_start,
                    "an ELSE expression",
                )?)?));
                cursor += 4;
                break;
            }
            Keyword::End => {
                cursor += 3;
                break;
            }
            Keyword::Then => return Err(SqlError::new("CASE has an unexpected THEN".into())),
        }
    }
    if cursor != tokens.len() {
        return Err(SqlError::new("CASE has tokens after END".into()));
    }
    let end = tokens.last().expect("CASE has END").0 + 3;
    if !raw[end..].trim().is_empty() {
        return Err(SqlError::new("CASE has trailing input after END".into()));
    }
    Ok(Expr::Case {
        operand,
        branches,
        else_expr,
    })
}
