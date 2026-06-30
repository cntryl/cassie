use super::{
    parse_parenthesized_prefix, parse_projection_item, parse_query_source, CommonTableExpression,
    Expr, HashSet, OrderExpr, ParsedStatement, QuerySource, QueryStatement, SqlError,
};
use crate::sql::ast::{SelectSet, SelectStatement, SetOperator};
use crate::sql::parser::clauses::{
    find_top_level_keyword, parse_clauses, Clause, ClauseMatch, ClauseToken,
};
use crate::sql::parser::expr::{parse_expression, parse_order_by, split_csv, take_int};

pub(super) fn parse_select_statement(
    sql: &str,
    withs: Vec<CommonTableExpression>,
    recursive: bool,
) -> Result<ParsedStatement, SqlError> {
    ensure_select_statement(sql)?;

    let trimmed = sql.trim().trim_end_matches(';').trim();
    if let Some((set_pos, set_token, set_operator)) = find_set_operation(trimmed) {
        return parse_set_select_statement(
            trimmed,
            withs,
            recursive,
            set_pos,
            set_token,
            set_operator,
        );
    }

    let after_select = trimmed[6..].trim();
    let (mut select_part, rest, source) = split_select_projection_and_rest(after_select)?;
    let (distinct, distinct_on) = parse_distinct_clause(&mut select_part)?;
    let clauses = parse_clauses(&rest)?;
    let source = resolve_select_source(&rest, &clauses, source)?;
    let parsed_clauses = parse_select_clauses(&rest, &clauses)?;
    let projection = parse_projection_items(&select_part)?;
    let filter = parsed_clauses
        .where_sql
        .as_deref()
        .map(parse_expression)
        .transpose()?;
    let group_by = parse_group_by_clause(parsed_clauses.group_sql.as_deref())?;
    let having = parsed_clauses
        .having_sql
        .as_deref()
        .map(parse_expression)
        .transpose()?;
    let order = parsed_clauses
        .order_sql
        .as_deref()
        .map(parse_order_by)
        .transpose()?
        .unwrap_or_default();

    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::Select(SelectStatement {
            source,
            ctes: withs,
            recursive,
            distinct,
            distinct_on,
            projection,
            filter,
            group_by,
            having,
            order,
            limit: parsed_clauses.limit,
            offset: parsed_clauses.offset,
            set: None,
        }),
    })
}

pub(super) fn parse_set_select_statement(
    trimmed: &str,
    withs: Vec<CommonTableExpression>,
    recursive: bool,
    set_pos: usize,
    set_token: &'static str,
    operator: SetOperator,
) -> Result<ParsedStatement, SqlError> {
    let token_len = set_token.len();
    let left_sql = trimmed[..set_pos].trim();
    let right_sql = trimmed[set_pos + token_len..].trim();
    if left_sql.is_empty() || right_sql.is_empty() {
        return Err(SqlError(
            "set operation requires both SELECT operands".into(),
        ));
    }

    let (right_sql, global_order, global_limit, global_offset) =
        split_set_right_and_global_clauses(right_sql)?;
    let mut left = parse_select_statement(left_sql, withs, recursive)?;
    let right = parse_select_statement(&right_sql, Vec::new(), false)?;
    let QueryStatement::Select(left_select) = &mut left.statement else {
        return Err(SqlError("set operation requires SELECT operands".into()));
    };
    let QueryStatement::Select(right_select) = right.statement else {
        return Err(SqlError("set operation requires SELECT operands".into()));
    };
    left_select.set = Some(Box::new(SelectSet {
        operator,
        right: Box::new(right_select),
    }));
    left_select.order = global_order;
    left_select.limit = global_limit;
    left_select.offset = global_offset;
    Ok(left)
}

#[must_use]
pub(super) fn find_set_operation(sql: &str) -> Option<(usize, &'static str, SetOperator)> {
    [
        ("union all", SetOperator::UnionAll),
        ("intersect", SetOperator::Intersect),
        ("except", SetOperator::Except),
        ("union", SetOperator::Union),
    ]
    .into_iter()
    .filter_map(|(token, operator)| {
        find_top_level_keyword(sql, 0, token).map(|pos| (pos, token, operator))
    })
    .min_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| right.1.len().cmp(&left.1.len()))
    })
}

type ResultClauses = (Vec<OrderExpr>, Option<i64>, Option<i64>);
type SetRightAndResultClauses = (String, Vec<OrderExpr>, Option<i64>, Option<i64>);

pub(super) fn split_set_right_and_global_clauses(
    right_sql: &str,
) -> Result<SetRightAndResultClauses, SqlError> {
    let trimmed = right_sql.trim();
    ensure_select_statement(trimmed)?;

    let after_select = trimmed[6..].trim();
    let clauses = parse_clauses(after_select)?;
    let Some(global_start) = clauses
        .iter()
        .find(|clause| {
            matches!(
                clause.token,
                ClauseToken::Recognized(Clause::Order | Clause::Limit | Clause::Offset)
            )
        })
        .map(|clause| clause.position)
    else {
        return Ok((trimmed.to_string(), Vec::new(), None, None));
    };

    let right_without_global = format!("SELECT {}", after_select[..global_start].trim());
    let global_rest = after_select[global_start..].trim();
    let (order, limit, offset) = parse_global_result_clauses(global_rest)?;
    Ok((right_without_global, order, limit, offset))
}

pub(super) fn parse_global_result_clauses(rest: &str) -> Result<ResultClauses, SqlError> {
    let clauses = parse_clauses(rest)?;
    let mut order = Vec::new();
    let mut limit = None;
    let mut offset = None;
    let mut seen = HashSet::new();

    for (idx, clause) in clauses.iter().enumerate() {
        let next_pos = clauses
            .get(idx + 1)
            .map_or_else(|| rest.len(), |clause| clause.position);
        let ClauseToken::Recognized(kind) = clause.token else {
            return Err(SqlError(format!("unsupported clause '{}'", clause.text())));
        };
        if !matches!(kind, Clause::Order | Clause::Limit | Clause::Offset) {
            return Err(SqlError(format!(
                "unsupported global set operation clause '{}'",
                clause.text()
            )));
        }
        let start = clause.position + kind.token().len();
        let raw_value = rest[start..next_pos].trim();
        if raw_value.is_empty() {
            return Err(SqlError(format!(
                "missing value for clause '{}'",
                clause.text()
            )));
        }

        match kind {
            Clause::Order => {
                if !seen.insert("order by") {
                    return Err(SqlError("duplicate ORDER BY clause".into()));
                }
                order = parse_order_by(raw_value)?;
            }
            Clause::Limit => {
                if !seen.insert("limit") {
                    return Err(SqlError("duplicate LIMIT clause".into()));
                }
                limit = take_int(raw_value).map_err(|error| SqlError(error.to_string()))?;
            }
            Clause::Offset => {
                if !seen.insert("offset") {
                    return Err(SqlError("duplicate OFFSET clause".into()));
                }
                offset = take_int(raw_value).map_err(|error| SqlError(error.to_string()))?;
            }
            Clause::Where | Clause::Group | Clause::Having => unreachable!(),
        }
    }

    Ok((order, limit, offset))
}

fn ensure_select_statement(sql: &str) -> Result<(), SqlError> {
    if sql.to_lowercase().starts_with("select ") {
        Ok(())
    } else {
        Err(SqlError(
            "only SELECT statements are supported in this stage".into(),
        ))
    }
}

fn split_select_projection_and_rest(
    after_select: &str,
) -> Result<(String, String, Option<QuerySource>), SqlError> {
    let from_pos = find_top_level_keyword(after_select, 0, "from");
    if let Some(from_pos) = from_pos {
        let select_part = after_select[..from_pos].trim();
        if select_part.is_empty() {
            return Err(SqlError("missing projection in SELECT statement".into()));
        }

        let rest = after_select[from_pos + 4..].trim();
        if rest.is_empty() {
            return Err(SqlError("missing collection in FROM".into()));
        }

        return Ok((select_part.to_string(), rest.to_string(), None));
    }

    let clauses = parse_clauses(after_select)?;
    let first_clause = clauses
        .first()
        .map_or_else(|| after_select.len(), |clause| clause.position);
    let select_part = after_select[..first_clause].trim();
    if select_part.is_empty() {
        return Err(SqlError("missing projection in SELECT statement".into()));
    }

    let rest = after_select[first_clause..].trim();
    if !rest.is_empty() && clauses.is_empty() {
        return Err(SqlError("unexpected tokens after SELECT projection".into()));
    }

    Ok((
        select_part.to_string(),
        rest.to_string(),
        Some(QuerySource::SingleRow),
    ))
}

fn parse_distinct_clause(select_part: &mut String) -> Result<(bool, Vec<Expr>), SqlError> {
    let mut distinct_on = Vec::new();
    let select_part_lower = select_part.to_lowercase();
    if select_part_lower.starts_with("distinct on") {
        let after_distinct_on = select_part["distinct on".len()..].trim_start();
        let (raw_distinct_on, remainder) = parse_parenthesized_prefix(after_distinct_on)
            .ok_or_else(|| {
                SqlError("DISTINCT ON requires a parenthesized expression list".into())
            })?;
        if raw_distinct_on.trim().is_empty() {
            return Err(SqlError(
                "DISTINCT ON requires at least one expression".into(),
            ));
        }
        distinct_on = split_csv(&raw_distinct_on)
            .into_iter()
            .map(parse_expression)
            .collect::<Result<Vec<_>, _>>()?;
        *select_part = remainder.trim().to_string();
        if select_part.is_empty() {
            return Err(SqlError("missing projection in SELECT statement".into()));
        }
        if select_part.to_lowercase().starts_with("distinct") {
            return Err(SqlError("duplicate DISTINCT clause".into()));
        }
        return Ok((false, distinct_on));
    }

    if select_part_lower == "distinct" || select_part_lower.starts_with("distinct ") {
        *select_part = select_part["distinct".len()..].trim().to_string();
        return Ok((true, distinct_on));
    }

    Ok((false, distinct_on))
}

fn resolve_select_source(
    rest: &str,
    clauses: &[ClauseMatch],
    source: Option<QuerySource>,
) -> Result<QuerySource, SqlError> {
    if let Some(source) = source {
        return Ok(source);
    }

    let first_clause = clauses
        .first()
        .map_or_else(|| rest.len(), |clause| clause.position);
    let from_source = rest[..first_clause].trim();
    if from_source.is_empty() {
        return Err(SqlError("missing collection in FROM".into()));
    }
    parse_query_source(from_source)
}

fn parse_projection_items(select_part: &str) -> Result<Vec<super::SelectItem>, SqlError> {
    let projection_tokens: Vec<&str> = split_csv(select_part);
    let mut projection = Vec::with_capacity(projection_tokens.len());
    for token in projection_tokens {
        projection.push(parse_projection_item(token.trim())?);
    }
    Ok(projection)
}

fn parse_group_by_clause(raw: Option<&str>) -> Result<Vec<Expr>, SqlError> {
    raw.map(|raw| {
        split_csv(raw)
            .into_iter()
            .map(parse_expression)
            .collect::<Result<Vec<_>, _>>()
    })
    .transpose()
    .map(Option::unwrap_or_default)
}

struct ParsedSelectClauses {
    where_sql: Option<String>,
    group_sql: Option<String>,
    having_sql: Option<String>,
    order_sql: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

fn parse_select_clauses(
    rest: &str,
    clauses: &[ClauseMatch],
) -> Result<ParsedSelectClauses, SqlError> {
    let mut parsed = ParsedSelectClauses {
        where_sql: None,
        group_sql: None,
        having_sql: None,
        order_sql: None,
        limit: None,
        offset: None,
    };
    let mut seen = HashSet::new();

    for (idx, clause) in clauses.iter().enumerate() {
        let next_pos = clauses
            .get(idx + 1)
            .map_or_else(|| rest.len(), |clause| clause.position);
        let (_, raw_value) = clause_value(rest, clause, next_pos)?;

        match clause.token {
            ClauseToken::Unsupported(kind) => {
                return Err(SqlError(format!("unsupported clause '{kind}'")));
            }
            ClauseToken::Recognized(kind) => match kind {
                Clause::Where => assign_clause(
                    &mut seen,
                    "where",
                    &mut parsed.where_sql,
                    raw_value.to_string(),
                    "duplicate WHERE clause",
                )?,
                Clause::Group => {
                    if !seen.insert("group by") {
                        return Err(SqlError("duplicate GROUP BY clause".into()));
                    }
                    let lower = raw_value.to_lowercase();
                    if lower.contains("grouping sets")
                        || lower.contains("rollup")
                        || lower.contains("cube")
                    {
                        return Err(SqlError("unsupported GROUP BY syntax".into()));
                    }
                    parsed.group_sql = Some(raw_value.to_string());
                }
                Clause::Having => assign_clause(
                    &mut seen,
                    "having",
                    &mut parsed.having_sql,
                    raw_value.to_string(),
                    "duplicate HAVING clause",
                )?,
                Clause::Order => assign_clause(
                    &mut seen,
                    "order by",
                    &mut parsed.order_sql,
                    raw_value.to_string(),
                    "duplicate ORDER BY clause",
                )?,
                Clause::Limit => {
                    if !seen.insert("limit") {
                        return Err(SqlError("duplicate LIMIT clause".into()));
                    }
                    parsed.limit =
                        take_int(raw_value).map_err(|error| SqlError(error.to_string()))?;
                }
                Clause::Offset => {
                    if !seen.insert("offset") {
                        return Err(SqlError("duplicate OFFSET clause".into()));
                    }
                    parsed.offset =
                        take_int(raw_value).map_err(|error| SqlError(error.to_string()))?;
                }
            },
        }
    }

    Ok(parsed)
}

fn clause_value<'a>(
    rest: &'a str,
    clause: &ClauseMatch,
    next_pos: usize,
) -> Result<(&'a str, &'a str), SqlError> {
    let token_text = match clause.token {
        ClauseToken::Recognized(clause_kind) => clause_kind.token(),
        ClauseToken::Unsupported(kind) => kind,
    };
    let start = clause.position + token_text.len();
    if start > rest.len() || next_pos > rest.len() || start > next_pos {
        return Err(SqlError(format!(
            "unsupported or malformed clause placement: {}",
            clause.text()
        )));
    }

    let raw_value = rest[start..next_pos].trim();
    if raw_value.is_empty() {
        return Err(SqlError(format!(
            "missing value for clause '{}'",
            clause.text()
        )));
    }

    Ok((token_text, raw_value))
}

fn assign_clause(
    seen: &mut HashSet<&'static str>,
    key: &'static str,
    slot: &mut Option<String>,
    value: String,
    duplicate_error: &'static str,
) -> Result<(), SqlError> {
    if !seen.insert(key) {
        return Err(SqlError(duplicate_error.into()));
    }
    *slot = Some(value);
    Ok(())
}
