//! Resolve admitted qualified builtin spellings at generic operator precedence.
use super::depth;
use crate::sql::ast::{BinaryOp, UnaryNumericOp};
use crate::sql::SqlError;
use crate::types::IntegerOperation;

#[derive(Clone, Copy)]
pub(super) struct Span<'a> {
    pub start: usize,
    pub end: usize,
    symbol: &'a str,
}

#[derive(Clone, Copy)]
struct BinaryCandidate {
    span_index: usize,
    left_significant: usize,
}

struct ScanState<'a> {
    spans: Vec<Span<'a>>,
    binary_candidates: Vec<BinaryCandidate>,
    last_significant: Option<char>,
    last_significant_position: Option<usize>,
    last_was_qualified_operator: bool,
    cursor: usize,
}

/// Qualified operator spans collected once for an expression and reused by its recursive slices.
pub(super) struct Index<'a> {
    source: &'a str,
    spans: Vec<Span<'a>>,
    binary_candidates: Vec<BinaryCandidate>,
}

fn token(raw: &str, start: usize, keyword_end: usize) -> Result<Option<Span<'_>>, SqlError> {
    let open = keyword_end + super::super::lexical::separator_end(&raw[keyword_end..], 0);
    if !raw[open..].starts_with('(') {
        return Ok(None);
    }
    let close = super::super::lexical::matching_paren(raw, open)
        .ok_or_else(|| SqlError::syntax("unterminated qualified operator".into()))?;
    let body = super::super::lexical::trim_separators(&raw[open + 1..close]);
    let (dot, end) =
        super::super::clauses::find_top_level_keyword_span(body, 0, ".").ok_or_else(|| {
            SqlError::unsupported("unqualified OPERATOR lookup remains pending".into())
        })?;
    let schema = crate::sql::ColumnIdentifierPath::parse_sql(
        super::super::lexical::trim_separators(&body[..dot]),
    )
    .map_err(SqlError::syntax)?;
    let symbol = super::super::lexical::trim_separators(&body[end..]);
    if symbol.is_empty()
        || !symbol
            .chars()
            .all(super::super::operators::operator_character)
    {
        return Err(SqlError::syntax("invalid qualified operator symbol".into()));
    }
    if schema.is_qualified() || schema.lookup_key() != "pg_catalog" {
        return Err(SqlError::unsupported(
            "qualified operator namespace is outside the selected builtin contract".into(),
        ));
    }
    Ok(Some(Span {
        start,
        end: close + 1,
        symbol,
    }))
}

impl<'a> Index<'a> {
    pub(super) fn scan(source: &'a str) -> Result<Self, SqlError> {
        if !source
            .as_bytes()
            .windows("operator".len())
            .any(|candidate| candidate.eq_ignore_ascii_case(b"operator"))
        {
            return Ok(Self {
                source,
                spans: Vec::new(),
                binary_candidates: Vec::new(),
            });
        }
        let (spans, binary_candidates) = collect_spans(source)?;

        Ok(Self {
            source,
            spans,
            binary_candidates,
        })
    }

    pub(super) fn last_binary(&self, raw: &str) -> Option<Span<'a>> {
        if self.spans.is_empty() {
            return None;
        }
        let (range_start, range_end) = self
            .range(raw)
            .expect("qualified operator index is queried with a slice of its source");
        // Candidate spans are ordered and disjoint, so one end-offset lookup finds the last
        // candidate wholly inside the recursive slice. Its recorded left operand then determines
        // whether it is binary in that slice.
        let after = self
            .binary_candidates
            .partition_point(|candidate| self.spans[candidate.span_index].end <= range_end);
        let candidate = self.binary_candidates.get(after.checked_sub(1)?)?;
        if candidate.left_significant < range_start {
            return None;
        }
        let span = self.spans[candidate.span_index];
        Some(Span {
            start: span.start - range_start,
            end: span.end - range_start,
            symbol: span.symbol,
        })
    }

    fn range(&self, raw: &str) -> Option<(usize, usize)> {
        let source_start = self.source.as_ptr().addr();
        let range_start = raw.as_ptr().addr();
        let start = range_start.checked_sub(source_start)?;
        let end = start.checked_add(raw.len())?;
        self.source.get(start..end).map(|_| (start, end))
    }
}

fn collect_spans(source: &str) -> Result<(Vec<Span<'_>>, Vec<BinaryCandidate>), SqlError> {
    let mut state = ScanState {
        spans: Vec::new(),
        binary_candidates: Vec::new(),
        last_significant: None,
        last_significant_position: None,
        last_was_qualified_operator: false,
        cursor: 0,
    };
    let mut depth = 0usize;
    let mut brackets = 0usize;
    let mut cases = 0usize;

    while state.cursor < source.len() {
        if let Some(end) = super::super::lexical::quoted_end(source, state.cursor) {
            let character = source[state.cursor..end]
                .chars()
                .next_back()
                .expect("quoted SQL token has at least one character");
            state.last_significant = Some(character);
            state.last_significant_position = Some(end - character.len_utf8());
            state.last_was_qualified_operator = false;
            state.cursor = end;
            continue;
        }

        if cases == 0 && depth == 0 && brackets == 0 {
            if let Some(keyword_end) =
                super::super::lexical::pattern_end(source, state.cursor, "operator")
            {
                if scan_operator_keyword(source, keyword_end, &mut state)? {
                    continue;
                }
            }
        }

        let separator_end = super::super::lexical::separator_end(source, state.cursor);
        if separator_end > state.cursor {
            state.cursor = separator_end;
            continue;
        }
        if super::super::lexical::pattern_end(source, state.cursor, "case").is_some() {
            cases += 1;
        } else if super::super::lexical::pattern_end(source, state.cursor, "end").is_some() {
            cases = cases.saturating_sub(1);
        }

        let character = source[state.cursor..]
            .chars()
            .next()
            .expect("cursor is before the expression end");
        let end = state.cursor + character.len_utf8();
        match character {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            '[' => brackets += 1,
            ']' => brackets = brackets.saturating_sub(1),
            _ => {}
        }
        if !character.is_whitespace() {
            state.last_significant = Some(character);
            state.last_significant_position = Some(state.cursor);
            state.last_was_qualified_operator = false;
        }
        state.cursor = end;
    }

    Ok((state.spans, state.binary_candidates))
}

fn scan_operator_keyword<'a>(
    source: &'a str,
    keyword_end: usize,
    state: &mut ScanState<'a>,
) -> Result<bool, SqlError> {
    // A member named operator is an identifier, as in public.operator(2).
    if state.last_significant != Some('.') {
        if let Some(span) = token(source, state.cursor, keyword_end)? {
            if state.spans.len() == depth::MAX_POSTFIX_DEPTH {
                return Err(depth::limit_error());
            }
            let binary_left = match (state.last_significant, state.last_significant_position) {
                (Some(left), Some(position))
                    if !state.last_was_qualified_operator
                        && !super::super::operators::operator_character(left) =>
                {
                    Some(position)
                }
                _ => None,
            };
            if let Some(left_significant) = binary_left {
                state.binary_candidates.push(BinaryCandidate {
                    span_index: state.spans.len(),
                    left_significant,
                });
            }
            state.spans.push(span);
            state.last_significant = Some(')');
            state.last_significant_position = Some(span.end - 1);
            state.last_was_qualified_operator = true;
            state.cursor = span.end;
            return Ok(true);
        }
    }
    let character = source[state.cursor..keyword_end]
        .chars()
        .next_back()
        .expect("operator keyword has at least one character");
    state.last_significant = Some(character);
    state.last_significant_position = Some(keyword_end - character.len_utf8());
    state.last_was_qualified_operator = false;
    state.cursor = keyword_end;
    Ok(true)
}

pub(super) fn prefix(raw: &str) -> Result<Option<(&str, UnaryNumericOp)>, SqlError> {
    let Some(end) = super::super::lexical::pattern_end(raw, 0, "operator") else {
        return Ok(None);
    };
    let Some(span) = token(raw, 0, end)? else {
        return Ok(None);
    };
    let op = match span.symbol {
        "+" => UnaryNumericOp::Plus,
        "-" => UnaryNumericOp::Minus,
        "@" => UnaryNumericOp::Absolute,
        "~" => UnaryNumericOp::Complement,
        "|/" => UnaryNumericOp::SquareRoot,
        "||/" => UnaryNumericOp::CubeRoot,
        _ => return Err(unsupported_signature()),
    };
    Ok(Some((&raw[..span.end], op)))
}

pub(super) fn binary(span: Span<'_>) -> Result<BinaryOp, SqlError> {
    Ok(match span.symbol {
        "+" => BinaryOp::Add,
        "-" => BinaryOp::Sub,
        "*" => BinaryOp::Mul,
        "/" => BinaryOp::Div,
        "%" => BinaryOp::Modulo,
        "^" => BinaryOp::Power,
        "||" => BinaryOp::Concat,
        "=" => BinaryOp::Eq,
        "<>" | "!=" => BinaryOp::NotEq,
        "<" => BinaryOp::Lt,
        "<=" => BinaryOp::Lte,
        ">" => BinaryOp::Gt,
        ">=" => BinaryOp::Gte,
        "&" => BinaryOp::Bitwise {
            operation: IntegerOperation::And,
        },
        "|" => BinaryOp::Bitwise {
            operation: IntegerOperation::Or,
        },
        "#" => BinaryOp::Bitwise {
            operation: IntegerOperation::Xor,
        },
        "<<" => BinaryOp::Bitwise {
            operation: IntegerOperation::Shl,
        },
        ">>" => BinaryOp::Bitwise {
            operation: IntegerOperation::Shr,
        },
        _ => return Err(unsupported_signature()),
    })
}

fn unsupported_signature() -> SqlError {
    SqlError::unsupported("qualified operator signature remains pending".into())
}

#[cfg(test)]
#[path = "qualified_operator_tests.rs"]
mod tests;
