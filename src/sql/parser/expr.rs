#[path = "text_array_literals.rs"]
mod text_array_literals;
pub(super) fn parse_text_array_element(raw: &str) -> Result<Option<String>, SqlError> {
    string_literals::parse(raw)
}
use super::clauses::{split_top_level, strip_parentheses};
use super::identifiers::{normalize_identifier, parse_quoted_identifier_chain};
use super::schema::{parse_data_type, starts_with_keyword};
use super::{
    parse_statement, BinaryOp, Expr, FunctionCall, NullsOrder, OrderExpr, QueryStatement,
    SortDirection, SqlError,
};

#[path = "expr_case.rs"]
mod case;
#[path = "expression_depth.rs"]
pub(super) mod depth;
#[path = "generic_prefix.rs"]
mod generic_prefix;
#[path = "like_predicates.rs"]
mod like_predicates;
#[path = "numeric_literals.rs"]
mod numeric_literals;
#[path = "qualified_operators.rs"]
mod qualified_operators;
#[path = "row_predicates.rs"]
mod row_predicates;
#[path = "string_literals.rs"]
mod string_literals;
#[path = "truth_predicates.rs"]
mod truth_predicates;
#[path = "unary_numeric.rs"]
mod unary_numeric;

pub(super) use numeric_literals::is_numeric_mantissa;
pub(super) use string_literals::unicode_identifier_prefix;

pub(super) fn split_csv(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut cursor = 0;
    while let Some((position, end)) = super::clauses::find_top_level_keyword_span(s, cursor, ",") {
        out.push(&s[cursor..position]);
        cursor = end;
    }
    out.push(&s[cursor..]);
    out
}

pub(super) fn parse_function(raw: &str) -> Result<Option<FunctionCall>, SqlError> {
    super::aggregate_call::parse_function(raw)
}

pub(crate) fn parse_expression(raw: &str) -> Result<Expr, SqlError> {
    parse_or_expression(super::lexical::trim_separators(raw))
}

pub(super) fn parse_or_expression(raw: &str) -> Result<Expr, SqlError> {
    if let Some((left, right)) = split_top_level(raw, " or ") {
        return Ok(Expr::Binary {
            left: Box::new(parse_or_expression(left)?),
            right: Box::new(parse_or_expression(right)?),
            op: BinaryOp::Or,
        });
    }

    parse_and_expression(raw)
}

pub(super) fn parse_and_expression(raw: &str) -> Result<Expr, SqlError> {
    if let Some((left, right)) = split_top_level_conjunction(raw) {
        return Ok(Expr::Binary {
            left: Box::new(parse_and_expression(left)?),
            right: Box::new(parse_and_expression(right)?),
            op: BinaryOp::And,
        });
    }

    parse_not_expression(raw)
}

pub(super) fn parse_not_expression(raw: &str) -> Result<Expr, SqlError> {
    let raw = super::lexical::trim_separators(raw);
    if starts_with_keyword(raw, "not") {
        let rest = raw["not".len()..].trim();
        if rest.is_empty() {
            return Err(SqlError::new("NOT requires an expression".into()));
        }
        return Ok(Expr::Not {
            expr: Box::new(parse_not_expression(rest)?),
        });
    }

    parse_comparison_expression(raw)
}

pub(super) fn parse_comparison_expression(raw: &str) -> Result<Expr, SqlError> {
    let raw = super::lexical::trim_separators(raw);

    if raw.starts_with('(') {
        let inner = strip_parentheses(raw);
        if let Some(inner) = inner {
            return parse_expression(inner);
        }
    }

    if let Some(truth) = truth_predicates::parse(raw)? {
        return Ok(truth);
    }
    for (suffix, negated) in [(" isnull", false), (" notnull", true)] {
        if let Some(operand) = super::lexical::strip_keyword_suffix(raw, suffix) {
            return Ok(Expr::IsNull {
                expr: Box::new(parse_comparison_expression(operand)?),
                negated,
            });
        }
    }

    for (token, op) in [
        (" is not distinct from ", BinaryOp::IsNotDistinctFrom),
        (" is distinct from ", BinaryOp::IsDistinctFrom),
    ] {
        if let Some((left, right)) = split_top_level(raw, token) {
            if [" is distinct from ", " is not distinct from "]
                .iter()
                .any(|token| {
                    split_top_level(left, token).is_some()
                        || split_top_level(right, token).is_some()
                })
            {
                return Err(SqlError::syntax(
                    "chained null-safe comparisons require parentheses".into(),
                ));
            }
            return row_predicates::comparison(left, op, right);
        }
    }

    if let Some((left, right)) = split_top_level(raw, " is not null") {
        if right.trim().is_empty() {
            return Ok(Expr::IsNull {
                expr: Box::new(parse_comparison_expression(left)?),
                negated: true,
            });
        }
    }
    if let Some((left, right)) = split_top_level(raw, " is null") {
        if right.trim().is_empty() {
            return Ok(Expr::IsNull {
                expr: Box::new(parse_comparison_expression(left)?),
                negated: false,
            });
        }
    }
    if let Some((left, right)) = split_top_level(raw, " not in ") {
        return parse_in_list_expression(left, right, true);
    }
    if let Some((left, right)) = split_top_level(raw, " in ") {
        return parse_in_list_expression(left, right, false);
    }
    if let Some((left, right)) = split_top_level(raw, " not between ") {
        return parse_between_expression(left, right, true);
    }
    if let Some((left, right)) = split_top_level(raw, " between ") {
        return parse_between_expression(left, right, false);
    }
    for (op, parsed) in [
        (" <=> ", BinaryOp::PgvectorCosine),
        (" <-> ", BinaryOp::PgvectorL2),
        (" <#> ", BinaryOp::PgvectorDot),
        ("<=", BinaryOp::Lte),
        (">=", BinaryOp::Gte),
        ("<>", BinaryOp::NotEq),
        ("!=", BinaryOp::NotEq),
        ("=", BinaryOp::Eq),
        ("<", BinaryOp::Lt),
        (">", BinaryOp::Gt),
    ] {
        let split = if op.starts_with(' ') {
            split_top_level(raw, op)
        } else {
            super::operators::split_operator(raw, op, false)
        };
        if let Some((left, right)) = split {
            return row_predicates::comparison(left, parsed, right);
        }
    }
    if let Some(predicate) = like_predicates::parse(raw)? {
        return Ok(predicate);
    }

    parse_prefix_expression(raw)
}

/// The "any other operator" tokens split at the prefix-expression level.
fn prefix_operators() -> [(&'static str, Option<BinaryOp>); 10] {
    use crate::types::extended::TypedOperator;
    use crate::types::IntegerOperation;
    [
        ("||", Some(BinaryOp::Concat)),
        (
            "|",
            Some(BinaryOp::Bitwise {
                operation: IntegerOperation::Or,
            }),
        ),
        (
            "&",
            Some(BinaryOp::Bitwise {
                operation: IntegerOperation::And,
            }),
        ),
        (
            "#",
            Some(BinaryOp::Bitwise {
                operation: IntegerOperation::Xor,
            }),
        ),
        (
            "<<",
            Some(BinaryOp::Bitwise {
                operation: IntegerOperation::Shl,
            }),
        ),
        (
            ">>",
            Some(BinaryOp::Bitwise {
                operation: IntegerOperation::Shr,
            }),
        ),
        ("^@", None),
        (
            "<<=",
            Some(BinaryOp::Typed {
                operator: TypedOperator::ContainedOrEqual,
            }),
        ),
        (
            ">>=",
            Some(BinaryOp::Typed {
                operator: TypedOperator::ContainsOrEqual,
            }),
        ),
        (
            "&&",
            Some(BinaryOp::Typed {
                operator: TypedOperator::Overlap,
            }),
        ),
    ]
}

fn parse_prefix_expression(raw: &str) -> Result<Expr, SqlError> {
    let operators = prefix_operators();
    let selected = operators
        .into_iter()
        // A cheap substring test skips the top-level scan for absent tokens.
        .filter(|(token, _)| raw.contains(token))
        .filter_map(|(token, op)| {
            super::operators::split_operator(raw, token, true)
                .map(|(left, right)| (left, right, op))
        })
        .max_by_key(|(left, _, _)| left.len());
    if let Some(span) = qualified_operators::last_binary(raw)? {
        if selected
            .as_ref()
            .is_none_or(|(left, _, _)| span.start > left.len())
        {
            let left = parse_prefix_expression(&raw[..span.start])?;
            let right = parse_prefix_expression(&raw[span.end..])?;
            depth::validate(&left, 1)?;
            depth::validate(&right, 1)?;
            return Ok(Expr::Binary {
                left: Box::new(left),
                right: Box::new(right),
                op: qualified_operators::binary(span)?,
            });
        }
    }
    if let Some((left, right, op)) = selected {
        let left = parse_prefix_expression(left)?;
        let right = parse_prefix_expression(right)?;
        return Ok(if let Some(op) = op {
            Expr::Binary {
                left: Box::new(left),
                right: Box::new(right),
                op,
            }
        } else {
            Expr::Function(Box::new(FunctionCall {
                name: "pg_catalog.starts_with".into(),
                args: vec![left, right],
                quantifier: crate::sql::ast::AggregateQuantifier::Default,
                filter: None,
                order_by: Vec::new(),
            }))
        });
    }
    if let Some(expression) = generic_prefix::parse(raw)? {
        return Ok(expression);
    }
    parse_arithmetic_expression(raw)
}

fn parse_arithmetic_expression(raw: &str) -> Result<Expr, SqlError> {
    if let Some((left, right, op)) =
        split_arithmetic_operator(raw, &[("+", BinaryOp::Add), ("-", BinaryOp::Sub)])
    {
        return Ok(Expr::Binary {
            left: Box::new(parse_prefix_expression(left)?),
            right: Box::new(parse_prefix_expression(right)?),
            op,
        });
    }
    if let Some((left, right, op)) = split_arithmetic_operator(
        raw,
        &[
            ("*", BinaryOp::Mul),
            ("/", BinaryOp::Div),
            ("%", BinaryOp::Modulo),
        ],
    ) {
        return Ok(Expr::Binary {
            left: Box::new(parse_prefix_expression(left)?),
            right: Box::new(parse_prefix_expression(right)?),
            op,
        });
    }
    if let Some((left, right)) = super::operators::split_arithmetic(raw, "^") {
        let operand = |expr: Expr| -> Result<Expr, SqlError> {
            if crate::sql::functions::has_unselected_numeric(&expr) {
                return Err(SqlError::unsupported(
                    "NUMERIC power remains pending under #793/#812; select FLOAT8 explicitly"
                        .into(),
                ));
            }
            Ok(if matches!(expr, Expr::StringLiteral(_)) {
                Expr::Cast {
                    expr: Box::new(expr),
                    data_type: crate::types::DataType::Float,
                    identity: crate::sql::ast::CastIdentity::Scalar,
                    source_type: None,
                }
            } else {
                expr
            })
        };
        return Ok(Expr::Binary {
            left: Box::new(operand(parse_prefix_expression(left)?)?),
            right: Box::new(operand(parse_prefix_expression(right)?)?),
            op: BinaryOp::Power,
        });
    }
    unary_numeric::parse(raw)
}

fn parse_postfix_expression(raw: &str) -> Result<Expr, SqlError> {
    let mut cast_positions = [0; depth::MAX_POSTFIX_DEPTH];
    let mut cast_count = 0;

    for position in super::lexical::top_level_cast_positions(raw) {
        if cast_count == depth::MAX_POSTFIX_DEPTH {
            return Err(depth::limit_error());
        }
        cast_positions[cast_count] = position;
        cast_count += 1;
    }
    let operand_end = if cast_count == 0 {
        raw.len()
    } else {
        cast_positions[0]
    };
    let operand_sql = super::lexical::trim_separators(&raw[..operand_end]);
    let selected_array = if cast_count > 0 {
        let end = if cast_count > 1 {
            cast_positions[1]
        } else {
            raw.len()
        };
        let target = parse_data_type(raw[cast_positions[0] + 2..end].trim())?;
        text_array_literals::parse(operand_sql, &target)?
    } else {
        None
    };
    let mut expr = selected_array.map_or_else(|| parse_expr_token(operand_sql), Ok)?;
    if cast_count > 0 {
        depth::validate(&expr, cast_count)?;
    }
    for (index, position) in cast_positions[..cast_count].iter().enumerate() {
        let type_end = if index + 1 == cast_count {
            raw.len()
        } else {
            cast_positions[index + 1]
        };
        let data_type = super::lexical::trim_separators(&raw[position + 2..type_end]);
        expr = Expr::Cast {
            expr: Box::new(expr),
            data_type: parse_data_type(data_type)?,
            source_type: None,
            identity: cast_identity(data_type),
        };
    }
    Ok(expr)
}

fn split_arithmetic_operator<'a>(
    raw: &'a str,
    operators: &[(&str, BinaryOp)],
) -> Option<(&'a str, &'a str, BinaryOp)> {
    operators
        .iter()
        .filter_map(|(operator, parsed)| {
            super::operators::split_arithmetic(raw, operator)
                .map(|(left, right)| (left, right, parsed.clone()))
        })
        .max_by_key(|(left, _, _)| left.len())
}

/// Splits `raw` at its first top-level `AND` conjunction, skipping each `AND`
/// that closes a preceding `[NOT] BETWEEN low AND high` predicate, so BETWEEN
/// binds tighter than AND as in PostgreSQL.
fn split_top_level_conjunction(raw: &str) -> Option<(&str, &str)> {
    const AND: &str = " and ";
    const BETWEEN: &str = " between ";

    let mut offset = 0;
    let mut open_betweens = 0usize;
    loop {
        let rest = &raw[offset..];
        let and_at = split_top_level(rest, AND).map(|(left, _)| left.len())?;
        let between_at = split_top_level(rest, BETWEEN).map(|(left, _)| left.len());
        if between_at.is_some_and(|between_at| between_at < and_at) {
            open_betweens += 1;
            offset = raw.len() - split_top_level(rest, BETWEEN)?.1.len();
        } else if open_betweens > 0 {
            open_betweens -= 1;
            offset = raw.len() - split_top_level(rest, AND)?.1.len();
        } else {
            let split_at = offset + and_at;
            return Some((&raw[..split_at], split_top_level(rest, AND)?.1));
        }
    }
}

pub(super) fn parse_between_expression(
    left: &str,
    right: &str,
    negated: bool,
) -> Result<Expr, SqlError> {
    let right = super::lexical::trim_separators(right);
    let symmetric = starts_with_keyword(right, "symmetric");
    let right = if symmetric {
        super::lexical::trim_separators(&right[9..])
    } else if starts_with_keyword(right, "asymmetric") {
        super::lexical::trim_separators(&right[10..])
    } else {
        right
    };
    let (low, high) = split_top_level(right, " and ")
        .ok_or_else(|| SqlError::new("BETWEEN predicate requires AND upper bound".into()))?;
    if high.trim().is_empty() {
        return Err(SqlError::new(
            "BETWEEN predicate requires an upper bound".to_string(),
        ));
    }

    let expr = parse_comparison_expression(left)?;
    let low = parse_comparison_expression(low)?;
    let high = parse_comparison_expression(high)?;
    for child in [&expr, &low, &high] {
        depth::validate(child, 1)?;
    }
    Ok(Expr::Between {
        expr: Box::new(expr),
        low: Box::new(low),
        high: Box::new(high),
        negated,
        symmetric,
    })
}

pub(super) fn parse_in_list_expression(
    left: &str,
    right: &str,
    negated: bool,
) -> Result<Expr, SqlError> {
    let values_raw = strip_parentheses(right.trim())
        .ok_or_else(|| SqlError::new("IN predicate requires a parenthesized value list".into()))?;
    if values_raw.trim().is_empty() {
        return Err(SqlError::new(
            "IN predicate requires at least one value".into(),
        ));
    }
    let values = split_csv(values_raw)
        .into_iter()
        .map(parse_expression)
        .collect::<Result<Vec<_>, _>>()?;
    if values.is_empty() {
        return Err(SqlError::new(
            "IN predicate requires at least one value".into(),
        ));
    }

    Ok(Expr::InList {
        expr: Box::new(parse_comparison_expression(left)?),
        values,
        negated,
    })
}

pub(super) fn parse_order_by(raw: &str) -> Result<Vec<OrderExpr>, SqlError> {
    let mut items = Vec::new();
    for token in split_csv(raw) {
        let token = super::lexical::trim_separators(token);
        let (token, nulls) =
            if let Some(token) = super::lexical::strip_keyword_suffix(token, " nulls first") {
                (token, Some(NullsOrder::First))
            } else if let Some(token) = super::lexical::strip_keyword_suffix(token, " nulls last") {
                (token, Some(NullsOrder::Last))
            } else {
                (token, None)
            };
        let (expr, direction) =
            if let Some(token) = super::lexical::strip_keyword_suffix(token, " desc") {
                (token, SortDirection::Desc)
            } else if let Some(token) = super::lexical::strip_keyword_suffix(token, " asc") {
                (token, SortDirection::Asc)
            } else {
                (token, SortDirection::Asc)
            };
        items.push(OrderExpr {
            expr: parse_expression(expr)?,
            direction,
            nulls,
        });
    }
    Ok(items)
}

pub(super) fn parse_expr_token(raw: &str) -> Result<Expr, SqlError> {
    let raw = super::lexical::trim_separators(raw);
    if raw.is_empty() {
        return Err(SqlError::new("invalid expression token".into()));
    }
    if let Some(inner) = strip_parentheses(raw) {
        return parse_expression(inner);
    }

    if let Some(value) = super::lexical::dollar_quoted_content(raw) {
        return Ok(Expr::StringLiteral(value.to_owned()));
    }
    if raw.starts_with('$') {
        let value = raw.trim_start_matches('$');
        if value.is_empty() {
            return Err(SqlError::new("invalid parameter index".into()));
        }

        let idx = value
            .parse::<usize>()
            .map_err(|_| SqlError::new(format!("invalid parameter index '{raw}'")))?;
        if idx == 0 {
            return Err(SqlError::new(format!("invalid parameter index '{raw}'")));
        }
        return Ok(Expr::Param(idx - 1));
    }
    if starts_with_keyword(raw, "case") {
        return case::parse_case(raw);
    }
    if raw.eq_ignore_ascii_case("null") {
        return Ok(Expr::Null);
    }
    if raw.eq_ignore_ascii_case("true") {
        return Ok(Expr::BoolLiteral(true));
    }
    if raw.eq_ignore_ascii_case("false") {
        return Ok(Expr::BoolLiteral(false));
    }
    if let Some(column) = parse_quoted_identifier_chain(raw)? {
        return Ok(Expr::Column(if column.is_qualified() {
            column.namespace_key()
        } else {
            column.lookup_key()
        }));
    }
    if let Some(value) = string_literals::parse(raw)? {
        return Ok(Expr::StringLiteral(value));
    }
    if let Some(value) = parse_typed_string_literal(raw)? {
        return Ok(value);
    }
    if let Some(value) = numeric_literals::parse(raw)? {
        return Ok(value);
    }
    if let Ok(value) = raw.parse::<f64>() {
        if !value.is_finite() {
            return Err(SqlError::new(format!(
                "numeric literal out of range '{raw}'"
            )));
        }
        return Ok(Expr::NumberLiteral(value));
    }
    if super::operators::numeric_exponent_prefix(raw) {
        return Err(SqlError::new(format!("invalid numeric literal '{raw}'")));
    }
    if let Some(exists) = parse_exists_expression(raw)? {
        return Ok(exists);
    }
    if let Some(cast) = parse_cast_expression(raw)? {
        return Ok(cast);
    }
    if raw != "*" && super::operators::contains_operator(raw) {
        return Err(SqlError::new(format!("invalid expression token '{raw}'")));
    }
    if let Some(func) = parse_function(raw)? {
        return Ok(Expr::Function(Box::new(func)));
    }

    if raw.chars().any(char::is_whitespace) {
        return Err(SqlError::new(format!("invalid expression token '{raw}'")));
    }

    let path = crate::sql::ColumnIdentifierPath::parse_sql(raw).map_err(SqlError::new)?;
    Ok(Expr::Column(path.lookup_key()))
}

fn parse_typed_string_literal(raw: &str) -> Result<Option<Expr>, SqlError> {
    let Some(quote) = raw.find('\'') else {
        return Ok(None);
    };
    if let Some(literal) = bit_string_literal(raw, quote)? {
        return Ok(Some(literal));
    }
    let Ok(data_type) = parse_data_type(raw[..quote].trim()) else {
        return Ok(None);
    };
    let Some(value) = string_literals::parse(&raw[quote..])? else {
        return Ok(None);
    };
    Ok(Some(Expr::Cast {
        expr: Box::new(Expr::StringLiteral(value)),
        data_type,
        source_type: None,
        identity: cast_identity(raw[..quote].trim()),
    }))
}

/// `B'1010'` and `X'1F'` bit-string constants, typed as unconstrained `bit`.
fn bit_string_literal(raw: &str, quote: usize) -> Result<Option<Expr>, SqlError> {
    let prefix = &raw[..quote];
    if !(prefix.eq_ignore_ascii_case("b") || prefix.eq_ignore_ascii_case("x")) {
        return Ok(None);
    }
    let Some(digits) = string_literals::parse(&raw[quote..])? else {
        return Ok(None);
    };
    let text = if prefix.eq_ignore_ascii_case("x") {
        format!("x{digits}")
    } else {
        digits
    };
    crate::types::extended::bit_string::parse_text(&text)
        .map_err(|error| SqlError::new(error.message))?;
    Ok(Some(Expr::Cast {
        expr: Box::new(Expr::StringLiteral(text)),
        data_type: crate::types::DataType::Extended(crate::types::extended::ExtendedType::Bit {
            length: None,
        }),
        source_type: None,
        identity: crate::sql::ast::CastIdentity::Scalar,
    }))
}

pub(super) fn parse_exists_expression(raw: &str) -> Result<Option<Expr>, SqlError> {
    let trimmed = raw.trim();
    if !starts_with_keyword(trimmed, "exists") {
        return Ok(None);
    }
    let inner = strip_parentheses(super::lexical::trim_separators(&trimmed[6..]))
        .ok_or_else(|| SqlError::new("EXISTS requires a parenthesized subquery".into()))?;
    let parsed = parse_statement(inner)?;
    if !matches!(parsed.statement, QueryStatement::Select(_)) {
        return Err(SqlError::new("EXISTS requires a SELECT subquery".into()));
    }

    Ok(Some(Expr::Exists(Box::new(parsed))))
}

pub(super) fn parse_cast_expression(raw: &str) -> Result<Option<Expr>, SqlError> {
    let trimmed = raw.trim();
    if !starts_with_keyword(trimmed, "cast") {
        return Ok(None);
    }
    let inner = strip_parentheses(super::lexical::trim_separators(&trimmed[4..]))
        .ok_or_else(|| SqlError::new("CAST requires parenthesized expression".into()))?;
    let (expr_raw, type_raw) = split_top_level(inner, " as ")
        .ok_or_else(|| SqlError::new("CAST requires AS type clause".into()))?;
    let data_type = parse_data_type(super::lexical::trim_separators(type_raw))?;

    Ok(Some(Expr::Cast {
        expr: Box::new(
            text_array_literals::parse(expr_raw, &data_type)?
                .map_or_else(|| parse_expression(expr_raw), Ok)?,
        ),
        data_type,
        source_type: None,
        identity: cast_identity(type_raw),
    }))
}

pub(super) fn parse_alias(raw: &str) -> (&str, Option<String>) {
    let token = raw.trim();
    if let Some((left, right)) = split_top_level(token, " as ") {
        if right.trim().is_empty() {
            return (token, None);
        }
        let alias = super::lexical::trim_separators(right);
        let alias = normalize_identifier(alias).unwrap_or_else(|_| alias.to_string());
        return (left.trim(), Some(alias));
    }
    (token, None)
}

fn cast_identity(raw: &str) -> crate::sql::ast::CastIdentity {
    let base = raw.split_once('(').map_or(raw, |(base, _)| base).trim();
    if base.eq_ignore_ascii_case("numeric") || base.eq_ignore_ascii_case("decimal") {
        crate::sql::ast::CastIdentity::Numeric
    } else {
        crate::sql::ast::CastIdentity::Scalar
    }
}
