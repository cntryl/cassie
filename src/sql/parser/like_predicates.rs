//! LIKE-family and regular-expression predicates.
use super::{parse_comparison_expression, split_top_level, Expr, SqlError};
use crate::sql::ast::{AggregateQuantifier, BinaryOp, FunctionCall};

#[derive(Clone, Copy)]
enum PatternKind {
    Like { case_insensitive: bool },
    Regex { case_insensitive: bool },
    Similar,
}

#[derive(Clone, Copy)]
struct PatternOperator {
    kind: PatternKind,
    negated: bool,
}

fn split_keyword(raw: &str) -> Option<(&str, &str, PatternOperator)> {
    [
        (" not similar to ", PatternKind::Similar, true),
        (" similar to ", PatternKind::Similar, false),
        (
            " not ilike ",
            PatternKind::Like {
                case_insensitive: true,
            },
            true,
        ),
        (
            " ilike ",
            PatternKind::Like {
                case_insensitive: true,
            },
            false,
        ),
        (
            " not like ",
            PatternKind::Like {
                case_insensitive: false,
            },
            true,
        ),
        (
            " like ",
            PatternKind::Like {
                case_insensitive: false,
            },
            false,
        ),
    ]
    .into_iter()
    .filter_map(|(keyword, kind, negated)| {
        split_top_level(raw, keyword)
            .map(|(left, right)| (left, right, PatternOperator { kind, negated }))
    })
    .min_by_key(|(left, _, _)| left.len())
}

fn split_symbol(raw: &str) -> Option<(&str, &str, PatternOperator)> {
    [
        (
            "!~~*",
            PatternKind::Like {
                case_insensitive: true,
            },
            true,
        ),
        (
            "~~*",
            PatternKind::Like {
                case_insensitive: true,
            },
            false,
        ),
        (
            "!~~",
            PatternKind::Like {
                case_insensitive: false,
            },
            true,
        ),
        (
            "~~",
            PatternKind::Like {
                case_insensitive: false,
            },
            false,
        ),
        (
            "!~*",
            PatternKind::Regex {
                case_insensitive: true,
            },
            true,
        ),
        (
            "~*",
            PatternKind::Regex {
                case_insensitive: true,
            },
            false,
        ),
        (
            "!~",
            PatternKind::Regex {
                case_insensitive: false,
            },
            true,
        ),
        (
            "~",
            PatternKind::Regex {
                case_insensitive: false,
            },
            false,
        ),
    ]
    .into_iter()
    .filter_map(|(token, kind, negated)| {
        super::super::operators::split_operator(raw, token, true)
            .map(|(left, right)| (left, right, PatternOperator { kind, negated }, token.len()))
    })
    .max_by_key(|(left, _, _, token_length)| (left.len(), *token_length))
    .map(|(left, right, operator, _)| (left, right, operator))
}

fn split_predicate(raw: &str) -> Option<(&str, &str, PatternOperator)> {
    split_keyword(raw).or_else(|| split_symbol(raw))
}

fn function(name: &str, arguments: Vec<Expr>) -> Expr {
    Expr::Function(Box::new(FunctionCall {
        name: name.to_owned(),
        args: arguments,
        quantifier: AggregateQuantifier::Default,
        filter: None,
        order_by: Vec::new(),
    }))
}

fn negate(expression: Expr, negated: bool) -> Expr {
    if negated {
        Expr::Not {
            expr: Box::new(expression),
        }
    } else {
        expression
    }
}

fn like_predicate(
    expression: Expr,
    pattern: Expr,
    escape: Option<Expr>,
    case_insensitive: bool,
) -> Expr {
    if case_insensitive {
        Expr::Like {
            expr: Box::new(expression),
            pattern: Box::new(pattern),
            escape: Box::new(escape.unwrap_or_else(|| Expr::StringLiteral("\\".to_owned()))),
            case_insensitive: true,
        }
    } else if let Some(escape) = escape {
        Expr::Like {
            expr: Box::new(expression),
            pattern: Box::new(pattern),
            escape: Box::new(escape),
            case_insensitive: false,
        }
    } else {
        Expr::Binary {
            left: Box::new(expression),
            op: BinaryOp::Like,
            right: Box::new(pattern),
        }
    }
}

fn regex_predicate(expression: Expr, pattern: Expr, case_insensitive: bool) -> Expr {
    let mut arguments = vec![expression, pattern];
    if case_insensitive {
        arguments.push(Expr::StringLiteral("i".to_owned()));
    }
    function("regexp_like", arguments)
}

fn similar_pattern(pattern: &str, escape: Option<&str>) -> Result<String, SqlError> {
    let escape = escape.unwrap_or("\\");
    let mut escape_chars = escape.chars();
    let escape = escape_chars.next();
    if escape_chars.next().is_some() {
        return Err(SqlError::syntax(
            "SIMILAR TO escape must be a single character".into(),
        ));
    }

    let mut translated = String::from("^(?:");
    let mut chars = pattern.chars();
    while let Some(character) = chars.next() {
        if Some(character) == escape {
            let escaped = chars.next().ok_or_else(|| {
                SqlError::syntax("SIMILAR TO pattern must not end with the escape character".into())
            })?;
            push_regex_literal(&mut translated, escaped);
        } else {
            match character {
                '%' => translated.push_str("(?s:.*)"),
                '_' => translated.push_str("(?s:.)"),
                '.' | '$' | '^' | '?' | '{' | '}' | '[' | ']' | '\\' => {
                    translated.push('\\');
                    translated.push(character);
                }
                _ => translated.push(character),
            }
        }
    }
    translated.push_str(")$");
    Ok(translated)
}

fn push_regex_literal(target: &mut String, character: char) {
    if ".^$*+?{}[]\\|()".contains(character) {
        target.push('\\');
    }
    target.push(character);
}

pub(super) fn parse(raw: &str) -> Result<Option<Expr>, SqlError> {
    let Some((left, right, operator)) = split_predicate(raw) else {
        return Ok(None);
    };
    if let Some((prefix, _, _)) = split_predicate(right) {
        let start = super::super::lexical::separator_end(right, prefix.len());
        let tail = &right[start..];
        let end = tail
            .find(|character: char| !character.is_ascii_alphabetic())
            .unwrap_or(tail.len());
        let token = &tail[..end];
        let message = if token.is_empty() {
            "chained pattern predicates require parentheses".to_owned()
        } else {
            format!("syntax error at or near \"{token}\"")
        };
        return Err(SqlError::syntax(message));
    }
    let expression = parse_comparison_expression(left)?;
    let predicate = match operator.kind {
        PatternKind::Like { case_insensitive } => {
            let (pattern, escape) =
                if let Some((pattern, escape)) = split_top_level(right, " escape ") {
                    (
                        parse_comparison_expression(pattern)?,
                        Some(parse_comparison_expression(escape)?),
                    )
                } else {
                    (parse_comparison_expression(right)?, None)
                };
            like_predicate(expression, pattern, escape, case_insensitive)
        }
        PatternKind::Regex { case_insensitive } => regex_predicate(
            expression,
            parse_comparison_expression(right)?,
            case_insensitive,
        ),
        PatternKind::Similar => {
            let (pattern, escape) =
                if let Some((pattern, escape)) = split_top_level(right, " escape ") {
                    (pattern, Some(escape))
                } else {
                    (right, None)
                };
            let pattern = parse_comparison_expression(pattern)?;
            if matches!(pattern, Expr::Null) {
                return Ok(Some(negate(
                    regex_predicate(expression, Expr::Null, false),
                    operator.negated,
                )));
            }
            let Expr::StringLiteral(pattern) = pattern else {
                return Err(SqlError::unsupported(
                    "SIMILAR TO currently requires a literal pattern".into(),
                ));
            };
            let escape = match escape.map(parse_comparison_expression).transpose()? {
                Some(Expr::Null) => {
                    return Ok(Some(negate(
                        regex_predicate(expression, Expr::Null, false),
                        operator.negated,
                    )));
                }
                Some(Expr::StringLiteral(value)) => Some(value),
                Some(_) => {
                    return Err(SqlError::unsupported(
                        "SIMILAR TO currently requires a literal escape".into(),
                    ));
                }
                None => None,
            };
            regex_predicate(
                expression,
                Expr::StringLiteral(similar_pattern(&pattern, escape.as_deref())?),
                false,
            )
        }
    };
    Ok(Some(negate(predicate, operator.negated)))
}
