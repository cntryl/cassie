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
struct SignificantRun {
    start: usize,
    end: usize,
}

/// Qualified operator spans collected once for an expression and reused by its recursive slices.
pub(super) struct Index<'a> {
    source: &'a str,
    spans: Vec<Span<'a>>,
    significant: Vec<SignificantRun>,
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
                significant: Vec::new(),
            });
        }

        let mut spans = Vec::new();
        let mut significant = None;
        let mut last_significant = None;
        let mut cursor = 0;
        let mut depth = 0usize;
        let mut brackets = 0usize;
        let mut cases = 0usize;

        while cursor < source.len() {
            if let Some(end) = super::super::lexical::quoted_end(source, cursor) {
                if let Some(runs) = &mut significant {
                    push_significant(runs, cursor, end);
                }
                last_significant = source[cursor..end].chars().next_back();
                cursor = end;
                continue;
            }

            if cases == 0 && depth == 0 && brackets == 0 {
                if let Some(keyword_end) =
                    super::super::lexical::pattern_end(source, cursor, "operator")
                {
                    // A member named operator is an identifier, as in public.operator(2).
                    let member = last_significant == Some('.');
                    if !member {
                        if let Some(span) = token(source, cursor, keyword_end)? {
                            if spans.len() == depth::MAX_POSTFIX_DEPTH {
                                return Err(depth::limit_error());
                            }
                            let runs = significant
                                .get_or_insert_with(|| scan_significant_runs(&source[..cursor]));
                            push_significant(runs, cursor, span.end);
                            spans.push(span);
                            last_significant = Some(')');
                            cursor = span.end;
                            continue;
                        }
                    }
                    if let Some(runs) = &mut significant {
                        push_significant(runs, cursor, keyword_end);
                    }
                    last_significant = source[..keyword_end].chars().next_back();
                    cursor = keyword_end;
                    continue;
                }
            }

            let separator_end = super::super::lexical::separator_end(source, cursor);
            if separator_end > cursor {
                cursor = separator_end;
                continue;
            }
            if super::super::lexical::pattern_end(source, cursor, "case").is_some() {
                cases += 1;
            } else if super::super::lexical::pattern_end(source, cursor, "end").is_some() {
                cases = cases.saturating_sub(1);
            }

            let character = source[cursor..]
                .chars()
                .next()
                .expect("cursor is before the expression end");
            let end = cursor + character.len_utf8();
            match character {
                '(' => depth += 1,
                ')' => depth = depth.saturating_sub(1),
                '[' => brackets += 1,
                ']' => brackets = brackets.saturating_sub(1),
                _ => {}
            }
            if !character.is_whitespace() {
                if let Some(runs) = &mut significant {
                    push_significant(runs, cursor, end);
                }
                last_significant = Some(character);
            }
            cursor = end;
        }

        Ok(Self {
            source,
            spans,
            significant: significant.unwrap_or_default(),
        })
    }

    pub(super) fn last_binary(&self, raw: &str) -> Option<Span<'a>> {
        if self.spans.is_empty() {
            return None;
        }
        let (range_start, range_end) = self
            .range(raw)
            .expect("qualified operator index is queried with a slice of its source");
        let first = self.spans.partition_point(|span| span.start < range_start);
        let after = self.spans.partition_point(|span| span.start < range_end);

        for span_index in (first..after).rev() {
            let span = self.spans[span_index];
            if span.end > range_end {
                continue;
            }
            let Some(left_end) =
                last_significant_char(self.source, &self.significant, range_start, span.start)
            else {
                continue;
            };
            let adjacent = span_index > first
                && !has_significant_between(
                    &self.significant,
                    self.spans[span_index - 1].end,
                    span.start,
                );
            if !super::super::operators::operator_character(left_end) && !adjacent {
                return Some(Span {
                    start: span.start - range_start,
                    end: span.end - range_start,
                    symbol: span.symbol,
                });
            }
        }
        None
    }

    fn range(&self, raw: &str) -> Option<(usize, usize)> {
        let source_start = self.source.as_ptr().addr();
        let range_start = raw.as_ptr().addr();
        let start = range_start.checked_sub(source_start)?;
        let end = start.checked_add(raw.len())?;
        self.source.get(start..end).map(|_| (start, end))
    }
}

fn push_significant(runs: &mut Vec<SignificantRun>, start: usize, end: usize) {
    if let Some(previous) = runs.last_mut() {
        if previous.end == start {
            previous.end = end;
            return;
        }
    }
    runs.push(SignificantRun { start, end });
}

fn scan_significant_runs(source: &str) -> Vec<SignificantRun> {
    let mut runs = Vec::new();
    let mut cursor = 0;
    while cursor < source.len() {
        if let Some(end) = super::super::lexical::quoted_end(source, cursor) {
            push_significant(&mut runs, cursor, end);
            cursor = end;
            continue;
        }
        let end = super::super::lexical::separator_end(source, cursor);
        if end > cursor {
            cursor = end;
            continue;
        }
        let character = source[cursor..]
            .chars()
            .next()
            .expect("cursor is before the expression end");
        let end = cursor + character.len_utf8();
        push_significant(&mut runs, cursor, end);
        cursor = end;
    }
    runs
}

fn last_significant_char(
    source: &str,
    runs: &[SignificantRun],
    range_start: usize,
    position: usize,
) -> Option<char> {
    let index = runs
        .partition_point(|run| run.start < position)
        .checked_sub(1)?;
    let run = runs[index];
    let start = run.start.max(range_start);
    let end = run.end.min(position);
    (start < end)
        .then(|| source.get(start..end)?.chars().next_back())
        .flatten()
}

fn has_significant_between(runs: &[SignificantRun], start: usize, end: usize) -> bool {
    if start >= end {
        return false;
    }
    let index = runs.partition_point(|run| run.start < end);
    index > 0 && runs[index - 1].end > start
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
