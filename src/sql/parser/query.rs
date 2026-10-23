use super::clauses::{
    find_top_level_keyword, find_top_level_union, split_top_level, strip_parentheses,
};
use super::expr::{parse_alias, parse_expression, parse_function, parse_order_by, split_csv};
use super::identifiers::parse_relation_path_prefix;
use super::{
    parse_statement, CommonTableExpression, CteQuery, Expr, HashSet, JoinKind, OrderExpr,
    ParsedStatement, QuerySource, QueryStatement, SelectItem, SqlError, WindowFunctionCall,
};
use crate::sql::ast::{
    AggregateQuantifier, SetOperator, WindowFrame, WindowFrameBound, WindowFrameExclusion,
    WindowFrameUnit,
};

#[path = "query_select.rs"]
mod query_select;

#[path = "relation_alias.rs"]
mod relation_alias;

pub(super) fn parse_select_statement(
    sql: &str,
    withs: Vec<CommonTableExpression>,
    recursive: bool,
) -> Result<ParsedStatement, SqlError> {
    query_select::parse_select_statement(sql, withs, recursive)
}

pub(super) fn parse_with_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let remainder = super::lexical::trim_separators(&sql[4..]);
    let mut recursive = false;
    let after_recursive = if let Some(end) = super::lexical::pattern_end(remainder, 0, "recursive")
    {
        recursive = true;
        super::lexical::trim_separators(&remainder[end..])
    } else {
        remainder
    };
    if let Some((cte_sql, main)) = super::with_statement::split_dml(after_recursive) {
        let ctes = parse_cte_definitions(cte_sql, recursive)?;
        return super::with_statement::parse(ctes, main);
    }

    let select_pos = ["select", "values"]
        .iter()
        .filter_map(|keyword| find_top_level_keyword(after_recursive, 0, keyword))
        .min();
    if let Some(merge_pos) = find_top_level_keyword(after_recursive, 0, "merge")
        .filter(|merge_pos| select_pos.is_none_or(|select_pos| *merge_pos < select_pos))
    {
        let cte_sql = after_recursive[..merge_pos].trim();
        let cte_defs = parse_cte_definitions(cte_sql, recursive)?;
        return super::merge::parse_merge_statement(&after_recursive[merge_pos..], cte_defs);
    }
    let select_pos =
        select_pos.ok_or_else(|| SqlError::new("missing SELECT after WITH clause".into()))?;

    let cte_sql = after_recursive[..select_pos].trim();
    if cte_sql.is_empty() {
        return Err(SqlError::new(
            "missing CTE definition in WITH clause".into(),
        ));
    }
    if super::lexical::pattern_end(&after_recursive[select_pos..], 0, "select").is_none()
        && super::lexical::pattern_end(&after_recursive[select_pos..], 0, "values").is_none()
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
    let raw = super::lexical::trim_separators(raw);
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
    if token.ends_with(".*") {
        let path = crate::sql::ColumnIdentifierPath::parse_sql(token).map_err(SqlError::new)?;
        if path.is_qualified() && path.final_name() == "*" {
            return Ok(SelectItem::Column {
                name: path.namespace_key(),
                alias: None,
            });
        }
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
        Expr::Function(function) => SelectItem::Function {
            function: *function,
            alias,
        },
        Expr::RowCompare(_)
        | Expr::UnaryNumeric { .. }
        | Expr::IntegerNegate { .. }
        | Expr::Cast { .. }
        | Expr::Case { .. }
        | Expr::Binary { .. }
        | Expr::BoolLiteral(_)
        | Expr::Null
        | Expr::NumberLiteral(_)
        | Expr::IntegerLiteral(_)
        | Expr::NumericLiteral(_)
        | Expr::Param(_)
        | Expr::StringLiteral(_)
        | Expr::IsNull { .. }
        | Expr::TruthTest { .. }
        | Expr::InList { .. }
        | Expr::Between { .. }
        | Expr::Like { .. }
        | Expr::Not { .. }
        | Expr::Exists(_) => SelectItem::Expr { expr, alias },
        Expr::Subquery(ref subquery) => SelectItem::Expr {
            alias: alias.or_else(|| super::expr::subquery_output_name(subquery)),
            expr,
        },
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
    let aggregate_window = matches!(
        function_name.as_str(),
        "count" | "sum" | "bool_and" | "bool_or"
    );
    let function = if aggregate_window {
        if function.is_distinct() || !function.order_by.is_empty() {
            return Err(SqlError::unsupported(
                "DISTINCT and aggregate-local ORDER BY are not supported on window aggregates"
                    .into(),
            ));
        }
        lower_window_filter(function, &function_name)?
    } else {
        if function.has_modifiers() {
            return Err(SqlError::unsupported(
                "aggregate modifiers on window functions are not yet supported".into(),
            ));
        }
        function
    };
    if !matches!(
        function_name.as_str(),
        "row_number"
            | "rank"
            | "dense_rank"
            | "lag"
            | "lead"
            | "first_value"
            | "last_value"
            | "count"
            | "sum"
            | "bool_and"
            | "bool_or"
            | "var_pop"
            | "var_samp"
            | "variance"
            | "stddev_pop"
            | "stddev_samp"
            | "stddev"
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
    if aggregate_window
        && (function.args.len() != 1
            || (matches!(function.args.as_slice(), [Expr::Column(name)] if name == "*")
                && function_name != "count"))
    {
        return Err(SqlError::syntax(format!(
            "{} window aggregate expects one valid argument",
            function.name
        )));
    }

    let over_body = strip_parentheses(over_raw.trim())
        .ok_or_else(|| SqlError::new("window function OVER clause requires parentheses".into()))?;
    let (partition_by, order_by, frame) = parse_window_spec(over_body)?;
    Ok(Some(WindowFunctionCall {
        name: function.name,
        args: function.args,
        partition_by,
        order_by,
        frame,
    }))
}

fn lower_window_filter(
    mut function: super::FunctionCall,
    function_name: &str,
) -> Result<super::FunctionCall, SqlError> {
    let count_star = function_name == "count"
        && matches!(function.args.as_slice(), [Expr::Column(name)] if name == "*");
    let Some(predicate) = function.filter.take() else {
        if count_star {
            function.args = vec![Expr::IntegerLiteral(1)];
        }
        return Ok(function);
    };
    let value = match function.args.as_slice() {
        [Expr::Column(name)] if name == "*" && count_star => Expr::IntegerLiteral(1),
        [argument] => argument.clone(),
        _ => {
            return Err(SqlError::syntax(format!(
                "{} window aggregate expects one argument",
                function.name
            )))
        }
    };
    function.args = vec![Expr::Case {
        operand: None,
        branches: vec![(*predicate, value)],
        else_expr: Some(Box::new(Expr::Null)),
    }];
    function.quantifier = AggregateQuantifier::Default;
    Ok(function)
}

type WindowSpec = (Vec<Expr>, Vec<OrderExpr>, Option<WindowFrame>);

pub(super) fn parse_window_spec(raw: &str) -> Result<WindowSpec, SqlError> {
    let raw = super::lexical::trim_separators(raw);
    if raw.is_empty() {
        return Ok((Vec::new(), Vec::new(), None));
    }

    let frame_start = [
        ("rows", WindowFrameUnit::Rows),
        ("range", WindowFrameUnit::Range),
        ("groups", WindowFrameUnit::Groups),
    ]
    .into_iter()
    .filter_map(|(keyword, unit)| {
        find_top_level_keyword(raw, 0, keyword).map(|position| (position, keyword, unit))
    })
    .min_by_key(|(position, _, _)| *position);

    let (spec_raw, frame) = if let Some((position, keyword, unit)) = frame_start {
        let frame_raw = super::lexical::trim_separators(&raw[position + keyword.len()..]);
        if frame_raw.is_empty() {
            return Err(SqlError::unsupported("window frame requires bounds".into()));
        }
        (&raw[..position], Some(parse_window_frame(frame_raw, unit)?))
    } else {
        (raw, None)
    };
    let spec_raw = super::lexical::trim_separators(spec_raw);
    if let Some(end) = super::lexical::pattern_end(spec_raw, 0, "partition by") {
        let rest = super::lexical::trim_separators(&spec_raw[end..]);
        if let Some((partition_raw, order_raw)) = split_top_level(rest, " order by ") {
            let partition_by = split_csv(partition_raw)
                .into_iter()
                .map(parse_expression)
                .collect::<Result<Vec<_>, _>>()?;
            return Ok((partition_by, parse_order_by(order_raw)?, frame));
        }
        let partition_by = split_csv(rest)
            .into_iter()
            .map(parse_expression)
            .collect::<Result<Vec<_>, _>>()?;
        return Ok((partition_by, Vec::new(), frame));
    }

    if let Some(end) = super::lexical::pattern_end(spec_raw, 0, "order by") {
        return Ok((Vec::new(), parse_order_by(&spec_raw[end..])?, frame));
    }

    Err(SqlError::unsupported(
        "unsupported window function syntax".into(),
    ))
}

fn parse_window_frame(raw: &str, unit: WindowFrameUnit) -> Result<WindowFrame, SqlError> {
    let lower = raw.to_ascii_lowercase();
    let (bounds, exclusion) = split_window_exclusion(&lower)?;
    let bounds = super::lexical::trim_separators(bounds);
    let (start_raw, end_raw) = if let Some(end) = super::lexical::pattern_end(bounds, 0, "between")
    {
        let body = super::lexical::trim_separators(&bounds[end..]);
        let (start, end) = split_top_level(body, " and ").ok_or_else(|| {
            SqlError::unsupported("ROWS BETWEEN requires start and end bounds".into())
        })?;
        (start.trim(), end.trim())
    } else {
        (bounds.trim(), "current row")
    };
    let start = parse_window_bound(start_raw)?;
    let end = parse_window_bound(end_raw)?;
    if !valid_window_bound_order(start, end) {
        return Err(SqlError::unsupported(
            "invalid window frame bounds: start must precede end".into(),
        ));
    }
    Ok(WindowFrame {
        unit,
        start,
        end,
        exclusion,
    })
}

fn split_window_exclusion(raw: &str) -> Result<(&str, WindowFrameExclusion), SqlError> {
    let Some(position) = find_top_level_keyword(raw, 0, "exclude") else {
        return Ok((raw, WindowFrameExclusion::NoOthers));
    };
    let keyword = super::lexical::without_comments(&raw[position + "exclude".len()..])
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let exclusion = match keyword.as_str() {
        "no others" => WindowFrameExclusion::NoOthers,
        "current row" => WindowFrameExclusion::CurrentRow,
        "group" => WindowFrameExclusion::Group,
        "ties" => WindowFrameExclusion::Ties,
        value => {
            return Err(SqlError::unsupported(format!(
                "invalid window frame exclusion '{value}'"
            )))
        }
    };
    Ok((raw[..position].trim(), exclusion))
}

fn parse_window_bound(raw: &str) -> Result<WindowFrameBound, SqlError> {
    let keyword = super::lexical::without_comments(raw)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let raw = keyword.as_str();
    match raw {
        "unbounded preceding" => Ok(WindowFrameBound::UnboundedPreceding),
        "current row" => Ok(WindowFrameBound::CurrentRow),
        "unbounded following" => Ok(WindowFrameBound::UnboundedFollowing),
        _ => {
            let (value, bound) = if let Some(value) = raw.strip_suffix(" preceding") {
                (value, true)
            } else if let Some(value) = raw.strip_suffix(" following") {
                (value, false)
            } else {
                return Err(SqlError::unsupported(format!(
                    "invalid window frame bound '{raw}'"
                )));
            };
            if value.trim_start().starts_with('-') {
                return Err(SqlError::unsupported(
                    "negative window frame offsets are unsupported".into(),
                ));
            }
            let value = value.trim().parse::<u64>().map_err(|_| {
                SqlError::unsupported(format!("invalid window frame offset '{value}'"))
            })?;
            Ok(if bound {
                WindowFrameBound::Preceding(value)
            } else {
                WindowFrameBound::Following(value)
            })
        }
    }
}

fn valid_window_bound_order(start: WindowFrameBound, end: WindowFrameBound) -> bool {
    if matches!(start, WindowFrameBound::UnboundedPreceding)
        || matches!(end, WindowFrameBound::UnboundedFollowing)
    {
        return true;
    }
    if matches!(
        start,
        WindowFrameBound::Preceding(_) | WindowFrameBound::CurrentRow
    ) && matches!(
        end,
        WindowFrameBound::CurrentRow | WindowFrameBound::Following(_)
    ) {
        return true;
    }
    match (start, end) {
        (WindowFrameBound::Preceding(start), WindowFrameBound::Preceding(end)) => start >= end,
        (WindowFrameBound::Following(start), WindowFrameBound::Following(end)) => start <= end,
        _ => false,
    }
}

pub(super) fn parse_query_source(raw: &str) -> Result<QuerySource, SqlError> {
    let raw = super::lexical::trim_separators(raw);
    if let Some(join) = rightmost_join(raw) {
        let left = raw[..join.position].trim();
        let right = raw[join.end..].trim();
        return match join.kind {
            SourceJoinKind::OuterApply => parse_apply_source(left, right, true),
            SourceJoinKind::CrossApply => parse_apply_source(left, right, false),
            SourceJoinKind::Join(kind) if kind == JoinKind::Cross => Ok(QuerySource::Join {
                left: Box::new(parse_query_source(left)?),
                right: Box::new(parse_query_source(right)?),
                kind,
                on: Expr::BoolLiteral(true),
            }),
            SourceJoinKind::Join(kind) => parse_join_source(left, right, kind),
        };
    }

    parse_single_query_source(raw)
}

#[cfg(test)]
mod window_aggregate_tests {
    use super::parse_window_function;

    #[test]
    fn should_reject_distinct_and_aggregate_ordering_on_window_aggregates() {
        // Arrange
        let distinct =
            "SUM(DISTINCT n) OVER (ORDER BY id ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW)";
        let ordered =
            "SUM(n ORDER BY n) OVER (ORDER BY id ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW)";

        // Act
        let distinct_error = parse_window_function(distinct).expect_err("DISTINCT is pending");
        let ordered_error =
            parse_window_function(ordered).expect_err("aggregate-local ordering is pending");

        // Assert
        assert_eq!(distinct_error.kind(), crate::sql::SqlErrorKind::Unsupported);
        assert_eq!(ordered_error.kind(), crate::sql::SqlErrorKind::Unsupported);
    }
}

#[derive(Clone, Copy)]
enum SourceJoinKind {
    OuterApply,
    CrossApply,
    Join(JoinKind),
}

#[derive(Clone, Copy)]
struct SourceJoinMatch {
    position: usize,
    end: usize,
    kind: SourceJoinKind,
}

fn rightmost_join(raw: &str) -> Option<SourceJoinMatch> {
    let candidates = [
        ("outer apply", SourceJoinKind::OuterApply),
        ("cross apply", SourceJoinKind::CrossApply),
        ("full outer join", SourceJoinKind::Join(JoinKind::Full)),
        ("full join", SourceJoinKind::Join(JoinKind::Full)),
        ("right outer join", SourceJoinKind::Join(JoinKind::Right)),
        ("right join", SourceJoinKind::Join(JoinKind::Right)),
        ("left outer join", SourceJoinKind::Join(JoinKind::Left)),
        ("left join", SourceJoinKind::Join(JoinKind::Left)),
        ("cross join", SourceJoinKind::Join(JoinKind::Cross)),
        ("inner join", SourceJoinKind::Join(JoinKind::Inner)),
        ("join", SourceJoinKind::Join(JoinKind::Inner)),
    ];
    let mut rightmost = None;
    for (token, kind) in candidates {
        let mut start = 0;
        while let Some((position, end)) =
            super::clauses::find_top_level_keyword_span(raw, start, token)
        {
            let candidate = SourceJoinMatch {
                position,
                end,
                kind,
            };
            if rightmost.is_none_or(|current: SourceJoinMatch| {
                end > current.end || (end == current.end && position < current.position)
            }) {
                rightmost = Some(candidate);
            }
            start = end;
        }
    }
    rightmost
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
    let raw = super::lexical::trim_separators(raw);
    if raw.is_empty() {
        return Err(SqlError::new("missing collection in FROM".into()));
    }

    let lateral_end = super::lexical::pattern_end(raw, 0, "lateral");
    let lateral = lateral_end.is_some();
    let raw = lateral_end.map_or(raw, |end| super::lexical::trim_separators(&raw[end..]));

    if raw.starts_with('(') {
        let close = matching_closing_paren(raw)
            .ok_or_else(|| SqlError::new("invalid FROM subquery syntax".into()))?;
        let subquery_sql = &raw[1..close];
        let alias_raw = super::lexical::trim_separators(&raw[close + 1..]);
        let alias = super::lexical::pattern_end(alias_raw, 0, "as").map_or(alias_raw, |end| {
            super::lexical::trim_separators(&alias_raw[end..])
        });
        let parsed = parse_statement(subquery_sql)?;
        let QueryStatement::Select(select) = parsed.statement else {
            return Err(SqlError::new(
                "FROM subquery must be a SELECT statement".into(),
            ));
        };
        if matches!(select.source, QuerySource::Values { .. })
            && select.ctes.is_empty()
            && select.order.is_empty()
            && select.limit.is_none()
            && select.offset.is_none()
            && select.set.is_none()
        {
            if lateral {
                return Err(SqlError::unsupported(
                    "lateral VALUES is not yet supported".into(),
                ));
            }
            return relation_alias::parse(select.source, alias_raw);
        }
        if alias.contains('(') || alias.starts_with('"') {
            return relation_alias::parse(
                QuerySource::Subquery {
                    alias: "__derived_query".to_string(),
                    select,
                    lateral,
                },
                alias_raw,
            );
        }
        if alias.is_empty() || alias.split_whitespace().count() != 1 {
            return Err(SqlError::new(
                "FROM subquery requires a deterministic alias".into(),
            ));
        }

        return Ok(QuerySource::Subquery {
            alias: super::identifiers::normalize_identifier(alias)?,
            select,
            lateral,
        });
    }

    if let Some(open) = raw.find('(') {
        if crate::sql::functions::is_catalog_table(raw[..open].trim()) {
            let close = super::lexical::matching_paren(raw, open)
                .ok_or_else(|| SqlError::new("invalid catalog table-function call".into()))?;
            let function = parse_function(&raw[..=close])?
                .ok_or_else(|| SqlError::new("invalid catalog table-function call".into()))?;
            let name = function.name.to_ascii_lowercase();
            let lateral = lateral || crate::sql::functions::is_options_table(&name);
            return relation_alias::parse(
                QuerySource::TableFunction {
                    name,
                    function,
                    lateral,
                },
                &raw[close + 1..],
            );
        }
    }

    if let Some(function) = parse_function(raw)? {
        let lower_name = function.name.to_ascii_lowercase();
        if matches!(
            lower_name.as_str(),
            "graph_neighbors"
                | "graph_expand"
                | "graph_shortest_path"
                | "pg_show_all_settings"
                | "pg_catalog.pg_show_all_settings"
        ) || crate::sql::functions::is_catalog_table(&lower_name)
        {
            return Ok(QuerySource::TableFunction {
                name: lower_name,
                function,
                lateral,
            });
        }
    }

    let (path, rest) = parse_relation_path_prefix(raw)?;
    relation_alias::parse(QuerySource::Collection(path), rest)
}

pub(super) fn matching_closing_paren(raw: &str) -> Option<usize> {
    super::lexical::matching_paren(raw, 0)
}

pub(super) fn parse_cte_definitions(
    raw: &str,
    recursive: bool,
) -> Result<Vec<CommonTableExpression>, SqlError> {
    let mut out = Vec::new();
    for definition in split_csv(raw) {
        let definition = super::lexical::trim_separators(definition);
        if definition.is_empty() {
            continue;
        }

        let as_pos = find_top_level_keyword(definition, 0, "as").ok_or_else(|| {
            SqlError::new(format!("invalid CTE definition '{definition}': missing AS"))
        })?;
        let head = super::lexical::trim_separators(&definition[..as_pos]);
        let body = super::lexical::trim_separators(&definition[as_pos + 2..]);

        let (name, aliases) = parse_cte_header(head)?;
        let (materialization, body) = super::cte_materialization::parse(body);
        let body_sql = parse_enclosed_parenthesized(body)
            .ok_or_else(|| SqlError::new(format!("invalid CTE body for '{name}'")))?;
        // UNION is also valid in an ordinary CTE; the WITH keyword controls
        // whether its body may use the recursive representation.
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
        out.push(CommonTableExpression {
            name: name.clone(),
            aliases,
            query,
            materialization,
        });
    }

    if out.is_empty() {
        return Err(SqlError::new("empty WITH clause".into()));
    }

    Ok(out)
}

pub(super) fn parse_recursive_cte_query(body: &str) -> Result<Option<CteQuery>, SqlError> {
    let Some((set_pos, set_len, all)) = find_top_level_union(body) else {
        return Ok(None);
    };
    let operator = if all {
        SetOperator::UnionAll
    } else {
        SetOperator::Union
    };

    let base = body[..set_pos].trim();
    let recursive = body[(set_pos + set_len)..].trim();
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

pub(super) fn parse_cte_header(raw: &str) -> Result<(String, Vec<String>), SqlError> {
    let raw = super::lexical::trim_separators(raw);
    let open = raw.find('(').filter(|open| *open + 1 < raw.len());
    if let Some(open) = open {
        let close = raw
            .rfind(')')
            .ok_or_else(|| SqlError::new(format!("invalid CTE header '{raw}'")))?;
        if close <= open {
            return Err(SqlError::new(format!("invalid CTE header '{raw}'")));
        }

        let name = super::lexical::trim_separators(&raw[..open]);
        if name.is_empty() || name.contains('(') || name.contains(')') {
            return Err(SqlError::new(format!("invalid CTE header '{raw}'")));
        }

        if !super::lexical::trim_separators(&raw[close + 1..]).is_empty() {
            return Err(SqlError::new(format!("invalid CTE header '{raw}'")));
        }

        let aliases = split_csv(&raw[(open + 1)..close])
            .into_iter()
            .map(super::lexical::trim_separators)
            .filter(|alias| !alias.is_empty())
            .map(|alias| {
                let column =
                    crate::sql::ColumnIdentifierPath::parse_sql(alias).map_err(SqlError::new)?;
                if column.is_qualified() {
                    return Err(SqlError::new(format!(
                        "invalid column identifier '{alias}'"
                    )));
                }
                Ok(column.declared_name())
            })
            .collect::<Result<Vec<_>, _>>()?;
        if aliases.is_empty() {
            return Err(SqlError::new(format!("invalid CTE header '{raw}'")));
        }

        Ok((super::identifiers::normalize_identifier(name)?, aliases))
    } else {
        if raw.contains('(') || raw.contains(')') {
            return Err(SqlError::new(format!("invalid CTE header '{raw}'")));
        }

        Ok((super::identifiers::normalize_identifier(raw)?, Vec::new()))
    }
}

pub(super) fn parse_enclosed_parenthesized(raw: &str) -> Option<String> {
    let raw = super::lexical::trim_separators(raw);
    let close = super::lexical::matching_paren(raw, 0)?;
    (close + 1 == raw.len()).then(|| raw[1..close].to_string())
}

pub(super) fn parse_parenthesized_prefix(raw: &str) -> Option<(String, &str)> {
    let raw = super::lexical::trim_separators(raw);
    let close = super::lexical::matching_paren(raw, 0)?;
    Some((raw[1..close].to_string(), &raw[close + 1..]))
}
