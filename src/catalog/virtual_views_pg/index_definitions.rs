use std::fmt::Write as _;

use crate::catalog::{local_name, relation_schema_name, IndexMeta};

pub(super) fn index_definition(index: &IndexMeta) -> String {
    let include = if index.include_fields.is_empty() {
        String::new()
    } else {
        format!(
            " INCLUDE ({})",
            index
                .normalized_include_fields()
                .iter()
                .map(|field| crate::catalog::object_identity::quote_identifier(field))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    let nulls_policy = if index.nulls_not_distinct() {
        " NULLS NOT DISTINCT"
    } else {
        ""
    };
    let predicate = index
        .predicate
        .as_ref()
        .map(|predicate| format!(" WHERE {}", format_stored_expression(predicate)))
        .unwrap_or_default();
    let storage_options = index
        .options
        .iter()
        .filter(|(name, _)| matches!(name.as_str(), "deduplicate_items" | "fillfactor"))
        .map(|(name, value)| format!("{name}='{}'", value.replace('\'', "''")))
        .collect::<Vec<_>>();
    let storage_options = if storage_options.is_empty() {
        String::new()
    } else {
        format!(" WITH ({})", storage_options.join(", "))
    };
    let fields = index.normalized_fields();
    let keys =
        fields
            .iter()
            .enumerate()
            .map(|(position, field)| {
                format_index_key(
                    index,
                    position,
                    &crate::catalog::object_identity::quote_identifier(field),
                )
            })
            .chain(index.normalized_expressions().iter().enumerate().map(
                |(position, expression)| {
                    format_index_key(
                        index,
                        fields.len() + position,
                        &format_stored_expression(expression),
                    )
                },
            ))
            .collect::<Vec<_>>()
            .join(", ");
    let qualified_table = format!(
        "{}.{}",
        crate::catalog::object_identity::quote_identifier(
            &relation_schema_name(&index.collection,)
        ),
        crate::catalog::object_identity::quote_identifier(&local_name(&index.collection))
    );
    format!(
        "CREATE {}INDEX {} ON {} USING {} ({})",
        if index.unique { "UNIQUE " } else { "" },
        crate::catalog::object_identity::quote_identifier(&local_name(&index.name)),
        qualified_table,
        index.access_method(),
        keys
    ) + &include
        + nulls_policy
        + &predicate
        + &storage_options
}

fn format_index_key(index: &IndexMeta, position: usize, key: &str) -> String {
    let mut formatted = key.to_string();
    if let Some(operator_class) = index.key_option(position, "operator_class") {
        formatted.push(' ');
        formatted.push_str(operator_class);
    }

    let descending = index
        .key_option(position, "order")
        .is_some_and(|order| order.eq_ignore_ascii_case("desc"));
    if descending {
        formatted.push_str(" DESC");
    }

    let nulls_first = index
        .key_option(position, "nulls")
        .map_or(descending, |nulls| nulls.eq_ignore_ascii_case("first"));
    if nulls_first != descending {
        formatted.push_str(if nulls_first {
            " NULLS FIRST"
        } else {
            " NULLS LAST"
        });
    }
    formatted
}

pub(super) fn format_stored_expression(raw: &str) -> String {
    serde_json::from_str::<crate::sql::ast::Expr>(raw)
        .ok()
        .and_then(|expression| format_index_expression(&expression))
        .unwrap_or_else(|| raw.to_string())
}

fn format_index_expression(expression: &crate::sql::ast::Expr) -> Option<String> {
    use crate::sql::ast::Expr;

    match expression {
        Expr::Column(name) => Some(crate::catalog::object_identity::quote_identifier(name)),
        Expr::Param(index) => Some(format!("${}", index + 1)),
        Expr::StringLiteral(value) => Some(format!("'{}'", value.replace('\'', "''"))),
        Expr::NumberLiteral(value) if value.is_finite() => Some(value.to_string()),
        Expr::IntegerLiteral(value) => Some(value.to_string()),
        Expr::NumericLiteral(value) => Some(value.clone()),
        Expr::BoolLiteral(value) => Some(value.to_string()),
        Expr::Null => Some("NULL".to_string()),
        Expr::Case {
            operand,
            branches,
            else_expr,
        } => format_index_case_expression(operand.as_deref(), branches, else_expr.as_deref()),
        Expr::Binary { left, op, right } => Some(format!(
            "({} {} {})",
            format_index_expression(left)?,
            index_binary_operator(op)?,
            format_index_expression(right)?
        )),
        Expr::IsNull { .. }
        | Expr::TruthTest { .. }
        | Expr::InList { .. }
        | Expr::Like { .. }
        | Expr::Between { .. }
        | Expr::Not { .. } => format_index_predicate(expression),
        Expr::Cast {
            expr, data_type, ..
        } => Some(format!(
            "({})::{}",
            format_index_expression(expr)?,
            data_type.type_name()
        )),
        Expr::UnaryNumeric { expr, op } => Some(format!(
            "{}({})",
            index_unary_operator(*op),
            format_index_expression(expr)?
        )),
        Expr::IntegerNegate { expr, .. } => Some(format!("-({})", format_index_expression(expr)?)),
        Expr::RowCompare(comparison) => Some(format!(
            "(({}) {} ({}))",
            comparison
                .left
                .iter()
                .map(format_index_expression)
                .collect::<Option<Vec<_>>>()?
                .join(", "),
            index_row_comparison_operator(comparison.op),
            comparison
                .right
                .iter()
                .map(format_index_expression)
                .collect::<Option<Vec<_>>>()?
                .join(", ")
        )),
        Expr::Function(function) => format_index_function(function),
        Expr::NumberLiteral(_) | Expr::Exists(_) | Expr::Subquery(_) => None,
    }
}

fn format_index_case_expression(
    operand: Option<&crate::sql::ast::Expr>,
    branches: &[(crate::sql::ast::Expr, crate::sql::ast::Expr)],
    else_expr: Option<&crate::sql::ast::Expr>,
) -> Option<String> {
    let mut parts = vec!["CASE".to_string()];
    if let Some(operand) = operand {
        parts.push(format_index_expression(operand)?);
    }
    for (condition, result) in branches {
        parts.push(format!(
            "WHEN {} THEN {}",
            format_index_expression(condition)?,
            format_index_expression(result)?
        ));
    }
    if let Some(else_expr) = else_expr {
        parts.push(format!("ELSE {}", format_index_expression(else_expr)?));
    }
    parts.push("END".to_string());
    Some(parts.join(" "))
}

fn format_index_predicate(expression: &crate::sql::ast::Expr) -> Option<String> {
    use crate::sql::ast::Expr;

    match expression {
        Expr::IsNull { expr, negated } => Some(format!(
            "({} IS{} NULL)",
            format_index_expression(expr)?,
            if *negated { " NOT" } else { "" }
        )),
        Expr::TruthTest {
            expr,
            truth,
            negated,
        } => Some(format!(
            "({} {})",
            format_index_expression(expr)?,
            truth.label(*negated)
        )),
        Expr::InList {
            expr,
            values,
            negated,
        } => Some(format!(
            "({} {}IN ({}))",
            format_index_expression(expr)?,
            if *negated { "NOT " } else { "" },
            values
                .iter()
                .map(format_index_expression)
                .collect::<Option<Vec<_>>>()?
                .join(", ")
        )),
        Expr::Like {
            expr,
            pattern,
            escape,
            case_insensitive,
        } => Some(format!(
            "({} {} {} ESCAPE {})",
            format_index_expression(expr)?,
            if *case_insensitive { "ILIKE" } else { "LIKE" },
            format_index_expression(pattern)?,
            format_index_expression(escape)?
        )),
        Expr::Between {
            expr,
            low,
            high,
            negated,
            symmetric,
        } => Some(format!(
            "({} {}BETWEEN {}{} AND {})",
            format_index_expression(expr)?,
            if *negated { "NOT " } else { "" },
            if *symmetric { "SYMMETRIC " } else { "" },
            format_index_expression(low)?,
            format_index_expression(high)?
        )),
        Expr::Not { expr } => Some(format!("(NOT {})", format_index_expression(expr)?)),
        _ => None,
    }
}

fn format_index_function(function: &crate::sql::ast::FunctionCall) -> Option<String> {
    let args = function
        .args
        .iter()
        .map(format_index_expression)
        .collect::<Option<Vec<_>>>()?;
    let quantifier = match &function.quantifier {
        crate::sql::ast::AggregateQuantifier::Default => "",
        crate::sql::ast::AggregateQuantifier::All => "ALL ",
        crate::sql::ast::AggregateQuantifier::Distinct => "DISTINCT ",
    };
    let ordering = function
        .order_by
        .iter()
        .map(format_index_order_expression)
        .collect::<Option<Vec<_>>>()?;
    let ordered_args = match (args.is_empty(), ordering.is_empty()) {
        (true, true) => String::new(),
        (false, true) => format!("{quantifier}{}", args.join(", ")),
        (true, false) => format!("ORDER BY {}", ordering.join(", ")),
        (false, false) => format!(
            "{quantifier}{} ORDER BY {}",
            args.join(", "),
            ordering.join(", ")
        ),
    };
    let mut formatted = format!("{}({ordered_args})", function.name.to_ascii_lowercase());
    if let Some(filter) = &function.filter {
        let filter = format_index_expression(filter)?;
        write!(formatted, " FILTER (WHERE {filter})").expect("write to a String");
    }
    Some(formatted)
}

fn format_index_order_expression(order: &crate::sql::ast::OrderExpr) -> Option<String> {
    let mut formatted = format_index_expression(&order.expr)?;
    formatted.push_str(match &order.direction {
        crate::sql::ast::SortDirection::Asc => " ASC",
        crate::sql::ast::SortDirection::Desc => " DESC",
    });
    if let Some(nulls) = &order.nulls {
        formatted.push_str(match nulls {
            crate::sql::ast::NullsOrder::First => " NULLS FIRST",
            crate::sql::ast::NullsOrder::Last => " NULLS LAST",
        });
    }
    Some(formatted)
}

fn index_unary_operator(operator: crate::sql::ast::UnaryNumericOp) -> &'static str {
    use crate::sql::ast::UnaryNumericOp;

    match operator {
        UnaryNumericOp::Complement => "~",
        UnaryNumericOp::Absolute => "@",
        UnaryNumericOp::SquareRoot => "|/",
        UnaryNumericOp::CubeRoot => "||/",
        UnaryNumericOp::Plus => "+",
        UnaryNumericOp::Minus => "-",
    }
}

fn index_row_comparison_operator(operator: crate::sql::ast::RowComparisonOp) -> &'static str {
    use crate::sql::ast::RowComparisonOp;

    match operator {
        RowComparisonOp::Eq => "=",
        RowComparisonOp::NotEq => "<>",
        RowComparisonOp::Lt => "<",
        RowComparisonOp::Lte => "<=",
        RowComparisonOp::Gt => ">",
        RowComparisonOp::Gte => ">=",
        RowComparisonOp::IsDistinctFrom => "IS DISTINCT FROM",
        RowComparisonOp::IsNotDistinctFrom => "IS NOT DISTINCT FROM",
    }
}

fn index_binary_operator(operator: &crate::sql::ast::BinaryOp) -> Option<String> {
    use crate::sql::ast::BinaryOp;

    Some(
        match operator {
            BinaryOp::Eq => "=",
            BinaryOp::NotEq => "<>",
            BinaryOp::IsDistinctFrom => "IS DISTINCT FROM",
            BinaryOp::IsNotDistinctFrom => "IS NOT DISTINCT FROM",
            BinaryOp::Lt => "<",
            BinaryOp::Lte => "<=",
            BinaryOp::Gt => ">",
            BinaryOp::Gte => ">=",
            BinaryOp::And => "AND",
            BinaryOp::Or => "OR",
            BinaryOp::Concat | BinaryOp::ByteaConcat => "||",
            BinaryOp::Add => "+",
            BinaryOp::Sub => "-",
            BinaryOp::Mul => "*",
            BinaryOp::Div => "/",
            BinaryOp::Modulo => "%",
            BinaryOp::Power => "^",
            BinaryOp::JsonbKeyExists => "?",
            BinaryOp::JsonbKeyExistsAny => "?|",
            BinaryOp::JsonbKeyExistsAll => "?&",
            BinaryOp::JsonbDeletePath => "#-",
            BinaryOp::Like => "LIKE",
            BinaryOp::Bitwise { operation } | BinaryOp::Integer { operation, .. } => {
                return Some(operation.symbol().to_string());
            }
            BinaryOp::PgvectorCosine => "<=>",
            BinaryOp::PgvectorL2 => "<->",
            BinaryOp::PgvectorDot => "<#>",
            BinaryOp::Quantified { comparison, .. } => {
                return index_binary_operator(comparison);
            }
            BinaryOp::Typed { operator } => match operator {
                crate::types::extended::TypedOperator::Add => "+",
                crate::types::extended::TypedOperator::Sub => "-",
                crate::types::extended::TypedOperator::And => "&",
                crate::types::extended::TypedOperator::Or => "|",
                crate::types::extended::TypedOperator::Xor => "#",
                crate::types::extended::TypedOperator::ShiftLeft => "<<",
                crate::types::extended::TypedOperator::ShiftRight => ">>",
                crate::types::extended::TypedOperator::Concat => "||",
                crate::types::extended::TypedOperator::ContainedOrEqual => "<<=",
                crate::types::extended::TypedOperator::ContainsOrEqual => ">>=",
                crate::types::extended::TypedOperator::Overlap => "&&",
            },
        }
        .to_string(),
    )
}
