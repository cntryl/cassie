use super::super::clauses::{find_top_level_keyword, find_top_level_union};
use super::super::expr::{parse_expression, split_csv};
use super::{parse_statement, CommonTableExpression, SqlError};
use crate::sql::ast::{CteCycle, CteQuery, CteSearch, CteSearchOrder, Expr, SetOperator};

pub(super) fn parse_cte_definitions(
    raw: &str,
    recursive: bool,
) -> Result<Vec<CommonTableExpression>, SqlError> {
    let mut output = Vec::new();
    for definition in split_cte_definitions(raw) {
        let definition = super::super::lexical::trim_separators(definition);
        if definition.is_empty() {
            continue;
        }
        let as_position = find_top_level_keyword(definition, 0, "as").ok_or_else(|| {
            SqlError::new(format!("invalid CTE definition '{definition}': missing AS"))
        })?;
        let (name, aliases) = parse_cte_header(&definition[..as_position])?;
        let body_start = super::super::lexical::trim_separators(&definition[as_position + 2..]);
        let (materialization, body_start) = super::super::cte_materialization::parse(body_start);
        let (body_sql, suffix) = parse_parenthesized_prefix(body_start)
            .ok_or_else(|| SqlError::new(format!("invalid CTE body for '{name}'")))?;
        let (search, cycle, remaining) = parse_search_cycle(suffix)?;
        if !remaining.trim().is_empty() {
            return Err(SqlError::new(format!(
                "invalid SEARCH or CYCLE clause for CTE '{name}'"
            )));
        }

        let recursive_query = if recursive {
            parse_recursive_cte_query(&body_sql)?
        } else {
            None
        };
        let query = if let Some(query) = recursive_query {
            query
        } else {
            let parsed_body = parse_statement(&body_sql).map_err(|error| {
                SqlError::new(format!("invalid CTE body for '{name}': {error}"))
            })?;
            CteQuery::Simple(Box::new(parsed_body))
        };
        if (search.is_some() || cycle.is_some()) && !matches!(query, CteQuery::Recursive { .. }) {
            return Err(SqlError::new(
                "SEARCH and CYCLE clauses require a recursive CTE".into(),
            ));
        }
        output.push(CommonTableExpression {
            name,
            aliases,
            query,
            materialization,
            search,
            cycle,
        });
    }
    if output.is_empty() {
        return Err(SqlError::new("empty WITH clause".into()));
    }
    Ok(output)
}

fn split_cte_definitions(raw: &str) -> Vec<&str> {
    let mut definitions = Vec::new();
    let mut start = 0;
    let mut cursor = 0;
    while let Some((position, end)) = super::super::lexical::find_top_level_span(raw, cursor, ",") {
        if looks_like_cte_definition(&raw[end..]) {
            definitions.push(&raw[start..position]);
            start = end;
        }
        cursor = end;
    }
    definitions.push(&raw[start..]);
    definitions
}

fn looks_like_cte_definition(raw: &str) -> bool {
    let candidate = super::super::lexical::trim_separators(raw);
    let Some(as_position) = find_top_level_keyword(candidate, 0, "as") else {
        return false;
    };
    if parse_cte_header(&candidate[..as_position]).is_err() {
        return false;
    }
    let body = super::super::lexical::trim_separators(&candidate[as_position + 2..]);
    let (_, body) = super::super::cte_materialization::parse(body);
    body.starts_with('(')
}

fn parse_search_cycle(
    suffix: &str,
) -> Result<(Option<CteSearch>, Option<CteCycle>, &str), SqlError> {
    let mut remaining = suffix.trim_start();
    let mut search = None;
    let mut cycle = None;
    loop {
        if let Some(after_search) = keyword_suffix(remaining, "search") {
            if search.is_some() {
                return Err(SqlError::new("duplicate SEARCH clause".into()));
            }
            let (parsed, rest) = parse_search(after_search)?;
            search = Some(parsed);
            remaining = rest.trim_start();
            continue;
        }
        if let Some(after_cycle) = keyword_suffix(remaining, "cycle") {
            if cycle.is_some() {
                return Err(SqlError::new("duplicate CYCLE clause".into()));
            }
            let (parsed, rest) = parse_cycle(after_cycle)?;
            cycle = Some(parsed);
            remaining = rest.trim_start();
            continue;
        }
        break;
    }
    Ok((search, cycle, remaining))
}

fn parse_search(raw: &str) -> Result<(CteSearch, &str), SqlError> {
    let (order, rest) = if let Some(rest) = keyword_suffix(raw, "depth") {
        (CteSearchOrder::DepthFirst, expect_keyword(rest, "first")?)
    } else if let Some(rest) = keyword_suffix(raw, "breadth") {
        (CteSearchOrder::BreadthFirst, expect_keyword(rest, "first")?)
    } else {
        return Err(SqlError::new(
            "SEARCH requires DEPTH FIRST or BREADTH FIRST".into(),
        ));
    };
    let rest = expect_keyword(rest, "by")?;
    let rest = rest.trim_start();
    let set_position = find_top_level_keyword(rest, 0, "set")
        .ok_or_else(|| SqlError::new("SEARCH clause is missing SET".into()))?;
    let by = parse_column_list(&rest[..set_position])?;
    let (sequence_column, remaining) = take_column_name(&rest[set_position + 3..])?;
    Ok((
        CteSearch {
            order,
            by,
            sequence_column,
        },
        remaining,
    ))
}

fn parse_cycle(raw: &str) -> Result<(CteCycle, &str), SqlError> {
    let raw = raw.trim_start();
    let set_position = find_top_level_keyword(raw, 0, "set")
        .ok_or_else(|| SqlError::new("CYCLE clause is missing SET".into()))?;
    let by = parse_column_list(&raw[..set_position])?;
    let (mark_column, rest) = take_column_name(&raw[set_position + 3..])?;
    let rest = rest.trim_start();
    let (mark_value, default_value, rest) = if let Some(after_to) = keyword_suffix(rest, "to") {
        let after_to = after_to.trim_start();
        let default_position = find_top_level_keyword(after_to, 0, "default")
            .ok_or_else(|| SqlError::new("CYCLE TO clause is missing DEFAULT".into()))?;
        let mark_value = parse_cycle_constant(after_to[..default_position].trim())?;
        let after_default = after_to[default_position + 7..].trim_start();
        let using_position = find_top_level_keyword(after_default, 0, "using")
            .ok_or_else(|| SqlError::new("CYCLE DEFAULT clause is missing USING".into()))?;
        let default_value = parse_cycle_constant(after_default[..using_position].trim())?;
        (
            mark_value,
            default_value,
            &after_default[using_position + 5..],
        )
    } else {
        let using_position = find_top_level_keyword(rest, 0, "using")
            .ok_or_else(|| SqlError::new("CYCLE clause is missing USING".into()))?;
        (
            parse_expression("TRUE")?,
            parse_expression("FALSE")?,
            &rest[using_position + 5..],
        )
    };
    let (path_column, remaining) = take_column_name(rest)?;
    Ok((
        CteCycle {
            by,
            mark_column,
            mark_value,
            default_value,
            path_column,
        },
        remaining,
    ))
}

fn parse_cycle_constant(raw: &str) -> Result<Expr, SqlError> {
    if raw.trim_start().starts_with(['+', '-']) {
        return Err(SqlError::new(
            "CYCLE mark and default values must be literal constants".into(),
        ));
    }
    let value = parse_expression(raw)?;
    if matches!(
        value,
        Expr::StringLiteral(_)
            | Expr::NumberLiteral(_)
            | Expr::NumericLiteral(_)
            | Expr::IntegerLiteral(_)
            | Expr::BoolLiteral(_)
            | Expr::Null
    ) {
        return Ok(value);
    }
    Err(SqlError::new(
        "CYCLE mark and default values must be literal constants".into(),
    ))
}

fn parse_column_list(raw: &str) -> Result<Vec<String>, SqlError> {
    let columns = split_csv(raw)
        .into_iter()
        .map(|column| parse_column_name(column.trim()))
        .collect::<Result<Vec<_>, _>>()?;
    if columns.is_empty() {
        return Err(SqlError::new(
            "SEARCH and CYCLE require at least one key".into(),
        ));
    }
    let mut unique = std::collections::HashSet::new();
    if columns
        .iter()
        .any(|column| !unique.insert(column.to_ascii_lowercase()))
    {
        return Err(SqlError::new("duplicate SEARCH or CYCLE key".into()));
    }
    Ok(columns)
}

fn parse_cte_header(raw: &str) -> Result<(String, Vec<String>), SqlError> {
    let raw = super::super::lexical::trim_separators(raw);
    let open = raw.find('(').filter(|open| *open + 1 < raw.len());
    if let Some(open) = open {
        let close = super::super::lexical::matching_paren(raw, open)
            .ok_or_else(|| SqlError::new(format!("invalid CTE header '{raw}'")))?;
        let name = super::super::lexical::trim_separators(&raw[..open]);
        if name.is_empty() || name.contains('(') || name.contains(')') {
            return Err(SqlError::new(format!("invalid CTE header '{raw}'")));
        }
        if !super::super::lexical::trim_separators(&raw[close + 1..]).is_empty() {
            return Err(SqlError::new(format!("invalid CTE header '{raw}'")));
        }
        let aliases = split_csv(&raw[(open + 1)..close])
            .into_iter()
            .map(|alias| parse_column_name(alias.trim()))
            .collect::<Result<Vec<_>, _>>()?;
        if aliases.is_empty() {
            return Err(SqlError::new(format!("invalid CTE header '{raw}'")));
        }
        Ok((
            super::super::identifiers::normalize_identifier(name)?,
            aliases,
        ))
    } else {
        if raw.contains('(') || raw.contains(')') {
            return Err(SqlError::new(format!("invalid CTE header '{raw}'")));
        }
        Ok((
            super::super::identifiers::normalize_identifier(raw)?,
            Vec::new(),
        ))
    }
}

fn parse_column_name(raw: &str) -> Result<String, SqlError> {
    let path = crate::sql::ColumnIdentifierPath::parse_sql(raw).map_err(SqlError::new)?;
    if path.is_qualified() || path.final_name() == "*" {
        return Err(SqlError::new(format!("invalid CTE column name '{raw}'")));
    }
    Ok(path.declared_name())
}

fn take_column_name(raw: &str) -> Result<(String, &str), SqlError> {
    let raw = raw.trim_start();
    if raw.is_empty() {
        return Err(SqlError::new("missing CTE column name".into()));
    }
    let end = if raw.starts_with('"') {
        let mut index = 1;
        while index < raw.len() {
            if raw.as_bytes()[index] == b'"' {
                if raw.as_bytes().get(index + 1) == Some(&b'"') {
                    index += 2;
                } else {
                    index += 1;
                    break;
                }
            } else {
                index += raw[index..].chars().next().map_or(0, char::len_utf8);
            }
        }
        index
    } else {
        raw.find(char::is_whitespace).unwrap_or(raw.len())
    };
    let token = &raw[..end];
    let name = parse_column_name(token)?;
    Ok((name, &raw[end..]))
}

fn keyword_suffix<'a>(raw: &'a str, keyword: &str) -> Option<&'a str> {
    let raw = raw.trim_start();
    let end = super::super::lexical::pattern_end(raw, 0, keyword)?;
    Some(&raw[end..])
}

fn expect_keyword<'a>(raw: &'a str, keyword: &str) -> Result<&'a str, SqlError> {
    keyword_suffix(raw, keyword)
        .ok_or_else(|| SqlError::new(format!("expected {keyword} in CTE clause")))
}

fn parse_parenthesized_prefix(raw: &str) -> Option<(String, &str)> {
    let raw = raw.trim_start();
    let close = super::super::lexical::matching_paren(raw, 0)?;
    Some((raw[1..close].to_string(), &raw[close + 1..]))
}

pub(in crate::sql::parser) fn parse_enclosed_parenthesized(raw: &str) -> Option<String> {
    let raw = super::super::lexical::trim_separators(raw);
    let close = super::super::lexical::matching_paren(raw, 0)?;
    (close + 1 == raw.len()).then(|| raw[1..close].to_string())
}

fn parse_recursive_cte_query(body: &str) -> Result<Option<CteQuery>, SqlError> {
    let Some((set_position, set_length, all)) = find_top_level_union(body) else {
        return Ok(None);
    };
    let operator = if all {
        SetOperator::UnionAll
    } else {
        SetOperator::Union
    };
    let base = body[..set_position].trim();
    let recursive = body[(set_position + set_length)..].trim();
    if base.is_empty() || recursive.is_empty() {
        return Err(SqlError::new(
            "recursive CTE requires anchor and recursive terms".into(),
        ));
    }
    let base = parse_statement(base)
        .map_err(|error| SqlError::new(format!("invalid recursive CTE anchor: {error}")))?;
    let recursive = parse_statement(recursive)
        .map_err(|error| SqlError::new(format!("invalid recursive CTE recursive term: {error}")))?;
    Ok(Some(CteQuery::Recursive {
        operator,
        base: Box::new(base),
        recursive: Box::new(recursive),
    }))
}
