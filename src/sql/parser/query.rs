use super::clauses::{find_top_level_keyword, split_top_level, strip_parentheses};
use super::expr::{
    parse_alias, parse_expression, parse_function, parse_order_by, split_csv,
    split_csv_quoted_by_space,
};
use super::{
    parse_statement, CommonTableExpression, CteQuery, Expr, HashSet, JoinKind, OrderExpr,
    ParsedStatement, QuerySource, QueryStatement, SelectItem, SqlError, WindowFunctionCall,
};

#[path = "query_select.rs"]
mod query_select;

pub(super) fn parse_select_statement(
    sql: &str,
    withs: Vec<CommonTableExpression>,
    recursive: bool,
) -> Result<ParsedStatement, SqlError> {
    query_select::parse_select_statement(sql, withs, recursive)
}

pub(super) fn parse_with_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let remainder = sql[4..].trim_start();
    let lower_remainder = remainder.to_lowercase();
    let mut recursive = false;
    let after_recursive = if lower_remainder.starts_with("recursive ") {
        recursive = true;
        remainder[10..].trim_start()
    } else {
        remainder
    };

    let select_pos = find_top_level_keyword(after_recursive, 0, "select")
        .ok_or_else(|| SqlError::new("missing SELECT after WITH clause".into()))?;

    let cte_sql = after_recursive[..select_pos].trim();
    if cte_sql.is_empty() {
        return Err(SqlError::new(
            "missing CTE definition in WITH clause".into(),
        ));
    }
    if !after_recursive[select_pos..]
        .to_lowercase()
        .starts_with("select ")
    {
        return Err(SqlError::new(
            "only SELECT statements are supported in this stage".into(),
        ));
    }

    let cte_defs = parse_cte_definitions(cte_sql, recursive)?;
    let main_select = &after_recursive[select_pos..];
    parse_select_statement(main_select, cte_defs, recursive)
}

pub(super) fn parse_projection_items(
    raw: &str,
) -> Result<Vec<crate::sql::ast::SelectItem>, SqlError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(SqlError::new("missing projection".into()));
    }

    let mut projection = Vec::new();
    for token in split_csv(raw) {
        projection.push(parse_projection_item(token)?);
    }

    Ok(projection)
}

pub(super) fn parse_projection_item(raw: &str) -> Result<SelectItem, SqlError> {
    let token = raw.trim();
    if token == "*" {
        return Ok(SelectItem::Wildcard);
    }

    let (expr_raw, alias) = parse_alias(token);
    if expr_raw.trim().is_empty() {
        return Err(SqlError::new("invalid projection item".into()));
    }

    if let Some(function) = parse_window_function(expr_raw)? {
        return Ok(SelectItem::WindowFunction { function, alias });
    }

    let expr = parse_expression(expr_raw)?;
    Ok(match expr {
        Expr::Function(function) => SelectItem::Function { function, alias },
        Expr::Cast { .. }
        | Expr::Binary { .. }
        | Expr::BoolLiteral(_)
        | Expr::Null
        | Expr::NumberLiteral(_)
        | Expr::Param(_)
        | Expr::StringLiteral(_)
        | Expr::IsNull { .. }
        | Expr::InList { .. }
        | Expr::Between { .. }
        | Expr::Not { .. }
        | Expr::Exists(_) => SelectItem::Expr { expr, alias },
        Expr::Column(name) => SelectItem::Column { name, alias },
    })
}

pub(super) fn parse_window_function(raw: &str) -> Result<Option<WindowFunctionCall>, SqlError> {
    let Some((function_raw, over_raw)) = split_top_level(raw, " over ") else {
        return Ok(None);
    };
    let function = parse_function(function_raw.trim())?
        .ok_or_else(|| SqlError::new("window function requires function call".into()))?;
    let function_name = function.name.to_ascii_lowercase();
    if !matches!(
        function_name.as_str(),
        "row_number" | "rank" | "dense_rank" | "lag" | "lead" | "first_value" | "last_value"
    ) {
        return Err(SqlError::new(format!(
            "unsupported window function '{}'",
            function.name
        )));
    }
    if matches!(function_name.as_str(), "row_number" | "rank" | "dense_rank")
        && !function.args.is_empty()
    {
        return Err(SqlError::new(format!(
            "{} window function expects no args",
            function.name
        )));
    }
    if matches!(
        function_name.as_str(),
        "lag" | "lead" | "first_value" | "last_value"
    ) && function.args.len() != 1
    {
        return Err(SqlError::new(format!(
            "{} window function expects one arg",
            function.name
        )));
    }

    let over_body = strip_parentheses(over_raw.trim())
        .ok_or_else(|| SqlError::new("window function OVER clause requires parentheses".into()))?;
    let (partition_by, order_by) = parse_window_spec(over_body)?;
    Ok(Some(WindowFunctionCall {
        name: function.name,
        args: function.args,
        partition_by,
        order_by,
    }))
}

pub(super) fn parse_window_spec(raw: &str) -> Result<(Vec<Expr>, Vec<OrderExpr>), SqlError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }

    let lower = raw.to_lowercase();
    if lower.starts_with("partition by ") {
        let rest = raw["partition by ".len()..].trim();
        if let Some((partition_raw, order_raw)) = split_top_level(rest, " order by ") {
            let partition_by = split_csv(partition_raw)
                .into_iter()
                .map(parse_expression)
                .collect::<Result<Vec<_>, _>>()?;
            return Ok((partition_by, parse_order_by(order_raw)?));
        }
        let partition_by = split_csv(rest)
            .into_iter()
            .map(parse_expression)
            .collect::<Result<Vec<_>, _>>()?;
        return Ok((partition_by, Vec::new()));
    }

    if lower.starts_with("order by ") {
        return Ok((Vec::new(), parse_order_by(&raw["order by ".len()..])?));
    }

    Err(SqlError::new("unsupported window function syntax".into()))
}

pub(super) fn parse_query_source(raw: &str) -> Result<QuerySource, SqlError> {
    let raw = raw.trim();
    if let Some((left, right)) = split_top_level(raw, " outer apply ") {
        return parse_apply_source(left, right, true);
    }
    if let Some((left, right)) = split_top_level(raw, " cross apply ") {
        return parse_apply_source(left, right, false);
    }
    if let Some((left, right)) = split_top_level(raw, " full outer join ") {
        return parse_join_source(left, right, JoinKind::Full);
    }
    if let Some((left, right)) = split_top_level(raw, " full join ") {
        return parse_join_source(left, right, JoinKind::Full);
    }
    if let Some((left, right)) = split_top_level(raw, " right join ") {
        return parse_join_source(left, right, JoinKind::Right);
    }
    if let Some((left, right)) = split_top_level(raw, " left join ") {
        return parse_join_source(left, right, JoinKind::Left);
    }
    if let Some((left, right)) = split_top_level(raw, " cross join ") {
        return Ok(QuerySource::Join {
            left: Box::new(parse_query_source(left)?),
            right: Box::new(parse_query_source(right)?),
            kind: JoinKind::Cross,
            on: Expr::BoolLiteral(true),
        });
    }
    if let Some((left, right)) = split_top_level(raw, " join ") {
        return parse_join_source(left, right, JoinKind::Inner);
    }

    parse_single_query_source(raw)
}

pub(super) fn parse_join_source(
    left: &str,
    right: &str,
    kind: JoinKind,
) -> Result<QuerySource, SqlError> {
    let (right, on) = split_top_level(right, " on ")
        .ok_or_else(|| SqlError::new("JOIN requires ON predicate".into()))?;
    Ok(QuerySource::Join {
        left: Box::new(parse_query_source(left)?),
        right: Box::new(parse_query_source(right)?),
        kind,
        on: parse_expression(on)?,
    })
}

pub(super) fn parse_apply_source(
    left: &str,
    right: &str,
    outer: bool,
) -> Result<QuerySource, SqlError> {
    let left = parse_query_source(left)?;
    let right = mark_source_lateral(parse_query_source(right)?);

    Ok(QuerySource::Join {
        left: Box::new(left),
        right: Box::new(right),
        kind: if outer {
            JoinKind::Left
        } else {
            JoinKind::Cross
        },
        on: Expr::BoolLiteral(true),
    })
}

pub(super) fn mark_source_lateral(source: QuerySource) -> QuerySource {
    match source {
        QuerySource::Subquery { alias, select, .. } => QuerySource::Subquery {
            alias,
            select,
            lateral: true,
        },
        QuerySource::Join {
            left,
            right,
            kind,
            on,
        } => QuerySource::Join {
            left,
            right: Box::new(mark_source_lateral(*right)),
            kind,
            on,
        },
        QuerySource::TableFunction { name, function, .. } => QuerySource::TableFunction {
            name,
            function,
            lateral: true,
        },
        other => other,
    }
}

pub(super) fn parse_single_query_source(raw: &str) -> Result<QuerySource, SqlError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(SqlError::new("missing collection in FROM".into()));
    }

    let lateral = raw.eq_ignore_ascii_case("lateral")
        || raw
            .get(..8)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("lateral "));
    let raw = if lateral { raw[7..].trim_start() } else { raw };

    if raw.starts_with('(') {
        let close = matching_closing_paren(raw)
            .ok_or_else(|| SqlError::new("invalid FROM subquery syntax".into()))?;
        let subquery_sql = &raw[1..close];
        let alias_raw = raw[close + 1..].trim();
        let alias = alias_raw
            .strip_prefix("AS ")
            .or_else(|| alias_raw.strip_prefix("as "))
            .unwrap_or(alias_raw)
            .trim();
        if alias.is_empty() || alias.split_whitespace().count() != 1 {
            return Err(SqlError::new(
                "FROM subquery requires a deterministic alias".into(),
            ));
        }

        let parsed = parse_statement(subquery_sql)?;
        let QueryStatement::Select(select) = parsed.statement else {
            return Err(SqlError::new(
                "FROM subquery must be a SELECT statement".into(),
            ));
        };
        return Ok(QuerySource::Subquery {
            alias: alias.to_string(),
            select: Box::new(select),
            lateral,
        });
    }

    if let Some(function) = parse_function(raw)? {
        let lower_name = function.name.to_ascii_lowercase();
        if matches!(
            lower_name.as_str(),
            "graph_neighbors" | "graph_expand" | "graph_shortest_path"
        ) {
            return Ok(QuerySource::TableFunction {
                name: lower_name,
                function,
                lateral,
            });
        }
    }

    let tokens = split_csv_quoted_by_space(raw);
    if tokens.len() != 1 {
        return Err(SqlError::new("unsupported FROM syntax".into()));
    }

    Ok(QuerySource::Collection(tokens[0].trim().to_string()))
}

pub(super) fn matching_closing_paren(raw: &str) -> Option<usize> {
    let mut depth = 0i32;
    let mut in_single = false;
    let mut in_double = false;
    for (index, ch) in raw.char_indices() {
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '(' if !in_single && !in_double => depth += 1,
            ')' if !in_single && !in_double => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

pub(super) fn parse_cte_definitions(
    raw: &str,
    recursive: bool,
) -> Result<Vec<CommonTableExpression>, SqlError> {
    let mut out = Vec::new();
    for definition in split_csv(raw) {
        let definition = definition.trim();
        if definition.is_empty() {
            continue;
        }

        let as_pos = find_top_level_keyword(definition, 0, "as").ok_or_else(|| {
            SqlError::new(format!("invalid CTE definition '{definition}': missing AS"))
        })?;
        let head = definition[..as_pos].trim();
        let body = definition[as_pos + 2..].trim();

        let (name, aliases) = parse_cte_header(head)?;
        let body_sql = parse_enclosed_parenthesized(body)
            .ok_or_else(|| SqlError::new(format!("invalid CTE body for '{name}'")))?;
        let query = if let Some(query) = parse_recursive_cte_query(&body_sql) {
            query
        } else {
            let parsed_body = parse_statement(&body_sql).map_err(|error| {
                SqlError::new(format!("invalid CTE body for '{name}': {error}"))
            })?;

            CteQuery::Simple(Box::new(parsed_body))
        };
        if recursive && !matches!(query, CteQuery::Recursive { .. }) {
            return Err(SqlError::new(format!(
                "recursive CTE '{name}' must include UNION ALL between anchor and recursive queries"
            )));
        }
        if !recursive && matches!(query, CteQuery::Recursive { .. }) {
            return Err(SqlError::new(
                "WITH clause is not marked RECURSIVE".to_string(),
            ));
        }

        out.push(CommonTableExpression {
            name: name.clone(),
            aliases,
            query,
        });
    }

    if out.is_empty() {
        return Err(SqlError::new("empty WITH clause".into()));
    }

    Ok(out)
}

pub(super) fn parse_recursive_cte_query(body: &str) -> Option<CteQuery> {
    let union_pos = find_top_level_keyword(body, 0, "union all")?;
    let base = body[..union_pos].trim();
    let recursive = body[(union_pos + "union all".len())..].trim();
    if base.is_empty() || recursive.is_empty() {
        return None;
    }

    Some(CteQuery::Recursive {
        base: Box::new(parse_statement(base).ok()?),
        recursive: Box::new(parse_statement(recursive).ok()?),
    })
}

pub(super) fn parse_cte_header(raw: &str) -> Result<(String, Vec<String>), SqlError> {
    let raw = raw.trim();
    let open = raw.find('(').filter(|open| *open + 1 < raw.len());
    if let Some(open) = open {
        let close = raw
            .rfind(')')
            .ok_or_else(|| SqlError::new(format!("invalid CTE header '{raw}'")))?;
        if close <= open {
            return Err(SqlError::new(format!("invalid CTE header '{raw}'")));
        }

        let name = raw[..open].trim();
        if name.is_empty() || name.contains('(') || name.contains(')') {
            return Err(SqlError::new(format!("invalid CTE header '{raw}'")));
        }

        if !raw[close + 1..].trim().is_empty() {
            return Err(SqlError::new(format!("invalid CTE header '{raw}'")));
        }

        let aliases = raw[(open + 1)..close]
            .split(',')
            .map(|alias| alias.trim().to_string())
            .filter(|alias| !alias.is_empty())
            .collect::<Vec<_>>();
        if aliases.is_empty() {
            return Err(SqlError::new(format!("invalid CTE header '{raw}'")));
        }

        Ok((name.to_string(), aliases))
    } else {
        if raw.contains('(') || raw.contains(')') {
            return Err(SqlError::new(format!("invalid CTE header '{raw}'")));
        }

        Ok((raw.to_string(), Vec::new()))
    }
}

pub(super) fn parse_enclosed_parenthesized(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if !raw.starts_with('(') || !raw.ends_with(')') {
        return None;
    }

    let mut depth = 0i32;
    let mut in_single = false;
    let mut in_double = false;
    for (i, ch) in raw.char_indices() {
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '(' if !in_single && !in_double => depth += 1,
            ')' if !in_single && !in_double => {
                depth -= 1;
                if depth == 0 && i != raw.len().saturating_sub(1) {
                    return None;
                }
            }
            _ => {}
        }
    }
    if depth != 0 {
        return None;
    }

    Some(raw[1..raw.len().saturating_sub(1)].to_string())
}

pub(super) fn parse_parenthesized_prefix(raw: &str) -> Option<(String, &str)> {
    let raw = raw.trim_start();
    if !raw.starts_with('(') {
        return None;
    }

    let mut depth = 0i32;
    let mut in_single = false;
    let mut in_double = false;
    for (index, ch) in raw.char_indices() {
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '(' if !in_single && !in_double => depth += 1,
            ')' if !in_single && !in_double => {
                depth -= 1;
                if depth == 0 {
                    return Some((raw[1..index].to_string(), &raw[index + 1..]));
                }
            }
            _ => {}
        }
    }

    None
}
