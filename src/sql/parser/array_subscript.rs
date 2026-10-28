//! PostgreSQL array subscript and slice postfix syntax.
use crate::sql::ast::{ArraySubscript, Expr};

use super::super::lexical;
use super::SqlError;

#[derive(Clone, Copy)]
struct BracketSpan {
    open: usize,
    close: usize,
}

pub(super) fn parse(raw: &str) -> Result<Option<Expr>, SqlError> {
    let spans = top_level_brackets(raw);
    let candidates = spans
        .into_iter()
        .filter(|span| {
            let prefix = lexical::trim_separators(&raw[..span.open]);
            let body = lexical::trim_separators(&raw[span.open + 1..span.close]);
            !body.is_empty() && !prefix.eq_ignore_ascii_case("array")
        })
        .collect::<Vec<_>>();
    let Some(last) = candidates.last() else {
        return Ok(None);
    };
    if !lexical::trim_separators(&raw[last.close + 1..]).is_empty() {
        return Ok(None);
    }

    let mut first = candidates.len() - 1;
    while first > 0 {
        let previous = candidates[first - 1];
        let current = candidates[first];
        if !lexical::trim_separators(&raw[previous.close + 1..current.open]).is_empty() {
            break;
        }
        first -= 1;
    }
    let suffix = &candidates[first..];
    let base = lexical::trim_separators(&raw[..suffix[0].open]);
    if base.is_empty() {
        return Ok(None);
    }
    let array = super::parse_expression(base)?;
    let mut subscripts = Vec::with_capacity(suffix.len());
    for span in suffix {
        let body = lexical::trim_separators(&raw[span.open + 1..span.close]);
        if let Some(position) = top_level_slice_colon(body) {
            let lower = lexical::trim_separators(&body[..position]);
            let upper = lexical::trim_separators(&body[position + 1..]);
            subscripts.push(ArraySubscript::Slice {
                lower: (!lower.is_empty())
                    .then(|| super::parse_expression(lower))
                    .transpose()?,
                upper: (!upper.is_empty())
                    .then(|| super::parse_expression(upper))
                    .transpose()?,
            });
        } else {
            subscripts.push(ArraySubscript::Index(super::parse_expression(body)?));
        }
    }
    Ok(Some(Expr::ArraySubscript {
        array: Box::new(array),
        subscripts,
    }))
}

fn top_level_brackets(input: &str) -> Vec<BracketSpan> {
    let mut spans = Vec::new();
    let mut index = 0;
    let mut parentheses = 0usize;
    let mut cases = 0usize;
    while index < input.len() {
        if let Some(end) = lexical::quoted_end(input, index) {
            index = end;
            continue;
        }
        if let Some(end) = lexical::comment_end(input, index) {
            index = end;
            continue;
        }
        if lexical::pattern_end(input, index, "case").is_some() {
            cases += 1;
        } else if lexical::pattern_end(input, index, "end").is_some() {
            cases = cases.saturating_sub(1);
        }
        match input.as_bytes()[index] {
            b'(' => parentheses += 1,
            b')' => parentheses = parentheses.saturating_sub(1),
            b'[' if parentheses == 0 => {
                let Some(close) = lexical::matching_bracket(input, index) else {
                    return spans;
                };
                if cases == 0 {
                    spans.push(BracketSpan { open: index, close });
                }
                index = close + 1;
                continue;
            }
            _ => {}
        }
        index += input[index..].chars().next().map_or(0, char::len_utf8);
    }
    spans
}

fn top_level_slice_colon(input: &str) -> Option<usize> {
    let mut cursor = 0;
    while let Some((position, end)) = lexical::find_top_level_span(input, cursor, ":") {
        let previous_is_colon = input[..position].ends_with(':');
        let next_is_colon = input[end..].starts_with(':');
        if !previous_is_colon && !next_is_colon {
            return Some(position);
        }
        cursor = end;
    }
    None
}
