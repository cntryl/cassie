//! Function-call structure, including aggregate-local query clauses.

use super::expr::{parse_expression, parse_order_by, split_csv};
use super::{lexical, FunctionCall, SqlError};
use crate::sql::ast::AggregateQuantifier;

#[path = "text_call.rs"]
mod text_call;

pub(super) fn parse_function(raw: &str) -> Result<Option<FunctionCall>, SqlError> {
    let Some((open, _)) = super::clauses::find_top_level_keyword_span(raw, 0, "(") else {
        return Ok(None);
    };
    let name = lexical::trim_separators(&raw[..open]);
    if name.is_empty() {
        return Ok(None);
    }
    let close = lexical::matching_paren(raw, open)
        .ok_or_else(|| SqlError::syntax("unterminated function call".into()))?;
    let tail = lexical::trim_separators(&raw[close + 1..]);
    let (within_group, tail) = parse_within_group(tail)?;
    let has_within_group = !within_group.is_empty();
    let filter = if tail.is_empty() {
        None
    } else {
        Some(Box::new(parse_filter(tail)?))
    };
    let mut body = lexical::trim_separators(&raw[open + 1..close]);
    let quantifier = if let Some(end) = lexical::pattern_end(body, 0, "distinct") {
        body = lexical::trim_separators(&body[end..]);
        if body.is_empty() || body == "*" {
            return Err(SqlError::syntax(
                "DISTINCT requires aggregate arguments".into(),
            ));
        }
        AggregateQuantifier::Distinct
    } else if let Some(end) = lexical::pattern_end(body, 0, "all") {
        body = lexical::trim_separators(&body[end..]);
        if body.is_empty() || body == "*" {
            return Err(SqlError::syntax("ALL requires aggregate arguments".into()));
        }
        AggregateQuantifier::All
    } else {
        AggregateQuantifier::Default
    };
    let (args_raw, aggregate_order_by) = if let Some((position, end)) =
        super::clauses::find_top_level_keyword_span(body, 0, "order by")
    {
        let order = lexical::trim_separators(&body[end..]);
        if order.is_empty() {
            return Err(SqlError::syntax("aggregate ORDER BY requires keys".into()));
        }
        (&body[..position], parse_order_by(order)?)
    } else {
        (body, Vec::new())
    };
    if has_within_group && !aggregate_order_by.is_empty() {
        return Err(SqlError::syntax(
            "WITHIN GROUP calls cannot also have aggregate-local ORDER BY".into(),
        ));
    }
    let order_by = if has_within_group {
        within_group
    } else {
        aggregate_order_by
    };
    let (resolved_name, args) = if let Some(call) = text_call::arguments(name, args_raw)? {
        call
    } else {
        let arguments = if lexical::trim_separators(args_raw).is_empty() {
            Vec::new()
        } else {
            split_csv(args_raw)
                .into_iter()
                .map(|raw| {
                    if lexical::trim_separators(raw).is_empty() {
                        return Err(SqlError::syntax("missing function argument".into()));
                    }
                    parse_expression(raw)
                })
                .collect::<Result<Vec<_>, _>>()?
        };
        (name.to_owned(), arguments)
    };
    if !order_by.is_empty() && lexical::trim_separators(args_raw) == "*" {
        return Err(SqlError::syntax(
            "aggregate star does not accept ORDER BY".into(),
        ));
    }
    if matches!(
        crate::sql::functions::function(&resolved_name).map(|metadata| metadata.id),
        Some(
            crate::sql::functions::FunctionId::PercentileCont
                | crate::sql::functions::FunctionId::PercentileDisc
        )
    ) && (!has_within_group || order_by.len() != 1)
    {
        return Err(SqlError::syntax(
            "percentile aggregates require one WITHIN GROUP ORDER BY key".into(),
        ));
    }
    Ok(Some(FunctionCall {
        name: resolved_name,
        args,
        quantifier,
        filter,
        order_by,
    }))
}

fn parse_within_group(raw: &str) -> Result<(Vec<super::OrderExpr>, &str), SqlError> {
    let Some(end) = lexical::pattern_end(raw, 0, "within group") else {
        return Ok((Vec::new(), raw));
    };
    let body = lexical::trim_separators(&raw[end..]);
    let close = lexical::matching_paren(body, 0)
        .ok_or_else(|| SqlError::syntax("WITHIN GROUP requires a parenthesized clause".into()))?;
    let clause = lexical::trim_separators(&body[1..close]);
    let order_end = lexical::pattern_end(clause, 0, "order by")
        .ok_or_else(|| SqlError::syntax("WITHIN GROUP requires ORDER BY".into()))?;
    let order = lexical::trim_separators(&clause[order_end..]);
    if order.is_empty() {
        return Err(SqlError::syntax(
            "WITHIN GROUP ORDER BY requires keys".into(),
        ));
    }
    Ok((
        parse_order_by(order)?,
        lexical::trim_separators(&body[close + 1..]),
    ))
}

fn parse_filter(raw: &str) -> Result<super::Expr, SqlError> {
    let end = lexical::pattern_end(raw, 0, "filter")
        .ok_or_else(|| SqlError::syntax("unexpected text after function call".into()))?;
    let body = lexical::trim_separators(&raw[end..]);
    let close = lexical::matching_paren(body, 0)
        .filter(|close| lexical::trim_separators(&body[close + 1..]).is_empty())
        .ok_or_else(|| SqlError::syntax("FILTER requires a parenthesized WHERE clause".into()))?;
    let body = lexical::trim_separators(&body[1..close]);
    let end = lexical::pattern_end(body, 0, "where")
        .ok_or_else(|| SqlError::syntax("FILTER requires WHERE".into()))?;
    let predicate = lexical::trim_separators(&body[end..]);
    if predicate.is_empty() {
        return Err(SqlError::syntax("FILTER requires a predicate".into()));
    }
    parse_expression(predicate)
}

#[cfg(test)]
mod tests {
    use super::parse_function;
    use crate::sql::ast::{Expr, FunctionCall, NullsOrder, SortDirection};

    #[test]
    fn should_retain_aggregate_clause_structure_given_distinct_filter_and_order_when_a_call_is_parsed(
    ) {
        // Arrange
        let sql = "SUM(DISTINCT amount ORDER BY amount DESC NULLS LAST) FILTER (WHERE included)";

        // Act
        let call = parse_function(sql)
            .expect("valid aggregate syntax")
            .expect("call");

        // Assert
        assert_eq!(call.name, "SUM");
        assert!(call.is_distinct());
        assert!(matches!(call.args.as_slice(), [Expr::Column(name)] if name == "amount"));
        assert!(matches!(call.filter.as_deref(), Some(Expr::Column(name)) if name == "included"));
        assert_eq!(call.order_by.len(), 1);
        assert!(matches!(call.order_by[0].direction, SortDirection::Desc));
        assert_eq!(call.order_by[0].nulls, Some(NullsOrder::Last));
    }

    #[test]
    fn should_parse_ordered_set_ordering_before_filter_given_a_percentile_call() {
        // Arrange
        let sql = "percentile_cont(0.25) WITHIN GROUP (ORDER BY amount DESC NULLS LAST) FILTER (WHERE included)";

        // Act
        let call = parse_function(sql)
            .expect("valid ordered-set aggregate syntax")
            .expect("function call");

        // Assert
        assert_eq!(call.name, "percentile_cont");
        assert!(
            matches!(
                call.args.as_slice(),
                [Expr::NumericLiteral(value)] if value == "0.25"
            ) || matches!(call.args.as_slice(), [Expr::NumberLiteral(value)] if value.to_bits() == 0.25_f64.to_bits())
        );
        assert!(matches!(call.filter.as_deref(), Some(Expr::Column(name)) if name == "included"));
        assert_eq!(call.order_by.len(), 1);
        assert!(matches!(call.order_by[0].expr, Expr::Column(ref name) if name == "amount"));
        assert!(matches!(call.order_by[0].direction, SortDirection::Desc));
        assert_eq!(call.order_by[0].nulls, Some(NullsOrder::Last));
    }

    #[test]
    fn should_preserve_legacy_function_json_given_no_modifiers_when_old_definitions_are_loaded() {
        // Arrange
        let legacy = r#"{"name":"lower","args":[{"Column":"label"}]}"#;

        // Act
        let call: FunctionCall = serde_json::from_str(legacy).expect("legacy JSON definition");
        let encoded = serde_json::to_string(&call).expect("stable legacy JSON definition");
        let legacy_expression = r#"{"Function":{"name":"lower","args":[{"Column":"label"}]}}"#;
        let expression: Expr =
            serde_json::from_str(legacy_expression).expect("legacy boxed function expression");

        // Assert
        assert!(!call.has_modifiers());
        assert_eq!(encoded, legacy);
        assert_eq!(
            serde_json::to_string(&expression).expect("transparent function box"),
            legacy_expression
        );
    }

    #[test]
    fn should_preserve_quoted_clause_words_given_nested_arguments_when_aggregate_options_are_parsed(
    ) {
        // Arrange
        let sql = "COUNT(DISTINCT COALESCE($tag$FILTER (WHERE false), ORDER BY$tag$, 'x')) FILTER (WHERE TRUE)";

        // Act
        let call = parse_function(sql)
            .expect("quoted words remain atomic")
            .expect("call");

        // Assert
        assert!(call.is_distinct());
        assert!(call.order_by.is_empty());
        assert!(matches!(
            call.filter.as_deref(),
            Some(Expr::BoolLiteral(true))
        ));
        assert!(
            matches!(call.args.as_slice(), [Expr::Function(function)] if function.args.len() == 2)
        );
    }

    #[test]
    fn should_reject_star_ordering_given_a_count_call_when_a_valid_filter_control_is_preserved() {
        // Arrange
        let invalid = "COUNT(* ORDER BY amount)";

        // Act
        let rejected = parse_function(invalid);
        let control = parse_function("COUNT(*) FILTER (WHERE TRUE)");

        // Assert
        assert_eq!(
            rejected
                .expect_err("star cannot take aggregate ordering")
                .kind(),
            crate::sql::SqlErrorKind::Syntax
        );
        let control = control.expect("star FILTER is valid").expect("count call");
        assert!(matches!(
            control.filter.as_deref(),
            Some(Expr::BoolLiteral(true))
        ));
    }
}
