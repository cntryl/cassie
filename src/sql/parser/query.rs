use super::clauses::*;
use super::expr::*;
use super::*;

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
        .ok_or_else(|| SqlError("missing SELECT after WITH clause".into()))?;

    let cte_sql = after_recursive[..select_pos].trim();
    if cte_sql.is_empty() {
        return Err(SqlError("missing CTE definition in WITH clause".into()));
    }
    if !after_recursive[select_pos..]
        .to_lowercase()
        .starts_with("select ")
    {
        return Err(SqlError(
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
        return Err(SqlError("missing projection".into()));
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
        return Err(SqlError("invalid projection item".into()));
    }

    if let Some(function) = parse_window_function(expr_raw)? {
        return Ok(SelectItem::WindowFunction { function, alias });
    }

    let expr = parse_expression(expr_raw)?;
    Ok(match expr {
        Expr::Function(function) => SelectItem::Function { function, alias },
        Expr::Cast { expr, data_type } => SelectItem::Function {
            function: FunctionCall {
                name: "CAST".to_string(),
                args: vec![*expr, Expr::StringLiteral(data_type.type_name())],
            },
            alias,
        },
        Expr::Column(name) => SelectItem::Column { name, alias },
        Expr::Binary { .. } => SelectItem::Expr { expr, alias },
        _ => {
            return Err(SqlError("unsupported projection expression".into()));
        }
    })
}

pub(super) fn parse_window_function(raw: &str) -> Result<Option<WindowFunctionCall>, SqlError> {
    let Some((function_raw, over_raw)) = split_top_level(raw, " over ") else {
        return Ok(None);
    };
    let function = parse_function(function_raw.trim())?
        .ok_or_else(|| SqlError("window function requires function call".into()))?;
    let function_name = function.name.to_ascii_lowercase();
    if !matches!(
        function_name.as_str(),
        "row_number" | "rank" | "dense_rank" | "lag" | "lead" | "first_value" | "last_value"
    ) {
        return Err(SqlError(format!(
            "unsupported window function '{}'",
            function.name
        )));
    }
    if matches!(function_name.as_str(), "row_number" | "rank" | "dense_rank")
        && !function.args.is_empty()
    {
        return Err(SqlError(format!(
            "{} window function expects no args",
            function.name
        )));
    }
    if matches!(
        function_name.as_str(),
        "lag" | "lead" | "first_value" | "last_value"
    ) && function.args.len() != 1
    {
        return Err(SqlError(format!(
            "{} window function expects one arg",
            function.name
        )));
    }

    let over_body = strip_parentheses(over_raw.trim())
        .ok_or_else(|| SqlError("window function OVER clause requires parentheses".into()))?;
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

    Err(SqlError("unsupported window function syntax".into()))
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
        .ok_or_else(|| SqlError("JOIN requires ON predicate".into()))?;
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
        return Err(SqlError("missing collection in FROM".into()));
    }

    let lateral = raw.eq_ignore_ascii_case("lateral")
        || raw
            .get(..8)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("lateral "));
    let raw = if lateral { raw[7..].trim_start() } else { raw };

    if raw.starts_with('(') {
        let close = matching_closing_paren(raw)
            .ok_or_else(|| SqlError("invalid FROM subquery syntax".into()))?;
        let subquery_sql = &raw[1..close];
        let alias_raw = raw[close + 1..].trim();
        let alias = alias_raw
            .strip_prefix("AS ")
            .or_else(|| alias_raw.strip_prefix("as "))
            .unwrap_or(alias_raw)
            .trim();
        if alias.is_empty() || alias.split_whitespace().count() != 1 {
            return Err(SqlError(
                "FROM subquery requires a deterministic alias".into(),
            ));
        }

        let parsed = parse_statement(subquery_sql)?;
        let QueryStatement::Select(select) = parsed.statement else {
            return Err(SqlError("FROM subquery must be a SELECT statement".into()));
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
        return Err(SqlError("unsupported FROM syntax".into()));
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

pub(super) fn parse_select_statement(
    sql: &str,
    withs: Vec<CommonTableExpression>,
    recursive: bool,
) -> Result<ParsedStatement, SqlError> {
    if !sql.to_lowercase().starts_with("select ") {
        return Err(SqlError(
            "only SELECT statements are supported in this stage".into(),
        ));
    }

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
    let from_pos = find_top_level_keyword(after_select, 0, "from");
    let (mut select_part, rest, source) = if let Some(from_pos) = from_pos {
        let select_part = after_select[..from_pos].trim();
        if select_part.is_empty() {
            return Err(SqlError("missing projection in SELECT statement".into()));
        }

        let rest = after_select[from_pos + 4..].trim();
        if rest.is_empty() {
            return Err(SqlError("missing collection in FROM".into()));
        }

        (select_part.to_string(), rest.to_string(), None)
    } else {
        let clauses = parse_clauses(after_select)?;
        let first_clause = clauses
            .first()
            .map(|clause| clause.position)
            .unwrap_or_else(|| after_select.len());
        let select_part = after_select[..first_clause].trim();
        if select_part.is_empty() {
            return Err(SqlError("missing projection in SELECT statement".into()));
        }

        let rest = after_select[first_clause..].trim();
        if !rest.is_empty() && clauses.is_empty() {
            return Err(SqlError("unexpected tokens after SELECT projection".into()));
        }

        (
            select_part.to_string(),
            rest.to_string(),
            Some(QuerySource::SingleRow),
        )
    };

    let mut distinct_on = Vec::new();
    let mut select_part_lower = select_part.to_lowercase();
    let distinct = if select_part_lower.starts_with("distinct on") {
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
        select_part = remainder.trim().to_string();
        if select_part.is_empty() {
            return Err(SqlError("missing projection in SELECT statement".into()));
        }
        select_part_lower = select_part.to_lowercase();
        if select_part_lower.starts_with("distinct") {
            return Err(SqlError("duplicate DISTINCT clause".into()));
        }
        false
    } else if select_part_lower == "distinct" || select_part_lower.starts_with("distinct ") {
        select_part = select_part["distinct".len()..].trim().to_string();
        true
    } else {
        false
    };

    let clauses = parse_clauses(&rest)?;

    let first_clause = clauses
        .first()
        .map(|clause| clause.position)
        .unwrap_or_else(|| rest.len());
    let source = if let Some(source) = source {
        source
    } else {
        let from_source = rest[..first_clause].trim();
        if from_source.is_empty() {
            return Err(SqlError("missing collection in FROM".into()));
        }
        parse_query_source(from_source)?
    };

    let mut where_clause: Option<String> = None;
    let mut group_clause: Option<String> = None;
    let mut having_clause: Option<String> = None;
    let mut order_clause: Option<String> = None;
    let mut limit_clause: Option<i64> = None;
    let mut offset_clause: Option<i64> = None;

    let mut seen = HashSet::new();
    for (idx, clause) in clauses.iter().enumerate() {
        let next_pos = clauses
            .get(idx + 1)
            .map(|clause| clause.position)
            .unwrap_or_else(|| rest.len());

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

        match clause.token {
            ClauseToken::Unsupported(kind) => {
                return Err(SqlError(format!("unsupported clause '{}'", kind)));
            }
            ClauseToken::Recognized(kind) => match kind {
                Clause::Where => {
                    if !seen.insert("where") {
                        return Err(SqlError("duplicate WHERE clause".into()));
                    }
                    where_clause = Some(raw_value.to_string());
                }
                Clause::Group => {
                    if !seen.insert("group by") {
                        return Err(SqlError("duplicate GROUP BY clause".into()));
                    }
                    if raw_value.to_lowercase().contains("grouping sets")
                        || raw_value.to_lowercase().contains("rollup")
                        || raw_value.to_lowercase().contains("cube")
                    {
                        return Err(SqlError("unsupported GROUP BY syntax".into()));
                    }
                    group_clause = Some(raw_value.to_string());
                }
                Clause::Having => {
                    if !seen.insert("having") {
                        return Err(SqlError("duplicate HAVING clause".into()));
                    }
                    having_clause = Some(raw_value.to_string());
                }
                Clause::Order => {
                    if !seen.insert("order by") {
                        return Err(SqlError("duplicate ORDER BY clause".into()));
                    }
                    order_clause = Some(raw_value.to_string());
                }
                Clause::Limit => {
                    if !seen.insert("limit") {
                        return Err(SqlError("duplicate LIMIT clause".into()));
                    }
                    limit_clause =
                        take_int(raw_value).map_err(|error| SqlError(error.to_string()))?;
                }
                Clause::Offset => {
                    if !seen.insert("offset") {
                        return Err(SqlError("duplicate OFFSET clause".into()));
                    }
                    offset_clause =
                        take_int(raw_value).map_err(|error| SqlError(error.to_string()))?;
                }
            },
        }
    }

    let projection_tokens: Vec<&str> = split_csv(&select_part);
    let mut projection = Vec::with_capacity(projection_tokens.len());
    for token in projection_tokens {
        let token = token.trim();
        projection.push(parse_projection_item(token)?);
    }

    let filter = where_clause.as_deref().map(parse_expression).transpose()?;
    let group_by = group_clause
        .as_deref()
        .map(|raw| {
            split_csv(raw)
                .into_iter()
                .map(parse_expression)
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?
        .unwrap_or_default();
    let having = having_clause.as_deref().map(parse_expression).transpose()?;
    let order = order_clause.as_deref().map(parse_order_by).transpose()?;

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
            order: order.unwrap_or_default(),
            limit: limit_clause,
            offset: offset_clause,
            set: None,
        }),
    })
}

pub(super) fn parse_set_select_statement(
    trimmed: &str,
    withs: Vec<CommonTableExpression>,
    recursive: bool,
    union_pos: usize,
    set_token: &'static str,
    operator: SetOperator,
) -> Result<ParsedStatement, SqlError> {
    let token_len = set_token.len();
    let left_sql = trimmed[..union_pos].trim();
    let right_sql = trimmed[union_pos + token_len..].trim();
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
    if !trimmed.to_lowercase().starts_with("select ") {
        return Err(SqlError("set operation requires SELECT operands".into()));
    }
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
            .map(|clause| clause.position)
            .unwrap_or_else(|| rest.len());
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
            SqlError(format!("invalid CTE definition '{definition}': missing AS"))
        })?;
        let head = definition[..as_pos].trim();
        let body = definition[as_pos + 2..].trim();

        let (name, aliases) = parse_cte_header(head)?;
        let body_sql = parse_enclosed_parenthesized(body)
            .ok_or_else(|| SqlError(format!("invalid CTE body for '{name}'")))?;
        let query = match parse_recursive_cte_query(&body_sql) {
            Some(query) => query,
            None => {
                let parsed_body = parse_statement(&body_sql).map_err(|error| {
                    SqlError(format!("invalid CTE body for '{name}': {}", error.0))
                })?;

                CteQuery::Simple(Box::new(parsed_body))
            }
        };
        if recursive && !matches!(query, CteQuery::Recursive { .. }) {
            return Err(SqlError(format!(
                "recursive CTE '{name}' must include UNION ALL between anchor and recursive queries"
            )));
        }
        if !recursive && matches!(query, CteQuery::Recursive { .. }) {
            return Err(SqlError("WITH clause is not marked RECURSIVE".to_string()));
        }

        out.push(CommonTableExpression {
            name: name.to_string(),
            aliases,
            query,
        });
    }

    if out.is_empty() {
        return Err(SqlError("empty WITH clause".into()));
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
            .ok_or_else(|| SqlError(format!("invalid CTE header '{raw}'")))?;
        if close <= open {
            return Err(SqlError(format!("invalid CTE header '{raw}'")));
        }

        let name = raw[..open].trim();
        if name.is_empty() || name.contains('(') || name.contains(')') {
            return Err(SqlError(format!("invalid CTE header '{raw}'")));
        }

        if !raw[close + 1..].trim().is_empty() {
            return Err(SqlError(format!("invalid CTE header '{raw}'")));
        }

        let aliases = raw[(open + 1)..close]
            .split(',')
            .map(|alias| alias.trim().to_string())
            .filter(|alias| !alias.is_empty())
            .collect::<Vec<_>>();
        if aliases.is_empty() {
            return Err(SqlError(format!("invalid CTE header '{raw}'")));
        }

        Ok((name.to_string(), aliases))
    } else {
        if raw.contains('(') || raw.contains(')') {
            return Err(SqlError(format!("invalid CTE header '{raw}'")));
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
