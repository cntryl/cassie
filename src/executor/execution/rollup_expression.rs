use crate::sql::ast::{BinaryOp, Expr, FunctionCall};

/// Formats an aggregate function as stable, parseable SQL for durable metadata.
pub(super) fn canonical_aggregate_expression(function: &FunctionCall) -> Result<String, String> {
    format_function(function)
}

fn format_expression(expression: &Expr) -> Result<String, String> {
    match expression {
        Expr::Column(name) if name == "*" => Ok("*".to_string()),
        Expr::Column(name) => Ok(quote_identifier(name)),
        Expr::Param(index) => Ok(format!("${}", index + 1)),
        Expr::StringLiteral(value) => Ok(format!("'{}'", value.replace('\'', "''"))),
        Expr::NumberLiteral(value) => format_float(*value),
        Expr::IntegerLiteral(value) => Ok(value.to_string()),
        Expr::BoolLiteral(value) => Ok(value.to_string()),
        Expr::Null => Ok("NULL".to_string()),
        Expr::Case {
            operand,
            branches,
            else_expr,
        } => {
            let mut sql = String::from("CASE");
            if let Some(operand) = operand {
                sql.push(' ');
                sql.push_str(&format_expression(operand)?);
            }
            for (when, then) in branches {
                sql.push_str(" WHEN ");
                sql.push_str(&format_expression(when)?);
                sql.push_str(" THEN ");
                sql.push_str(&format_expression(then)?);
            }
            if let Some(else_expr) = else_expr {
                sql.push_str(" ELSE ");
                sql.push_str(&format_expression(else_expr)?);
            }
            sql.push_str(" END");
            Ok(sql)
        }
        Expr::Binary { left, op, right } => Ok(format!(
            "({} {} {})",
            format_expression(left)?,
            format_binary_operator(op),
            format_expression(right)?
        )),
        Expr::IsNull { expr, negated } => Ok(format!(
            "({} IS{} NULL)",
            format_expression(expr)?,
            if *negated { " NOT" } else { "" }
        )),
        Expr::InList {
            expr,
            values,
            negated,
        } => {
            let values = values
                .iter()
                .map(format_expression)
                .collect::<Result<Vec<_>, _>>()?
                .join(", ");
            Ok(format!(
                "({}{} IN ({}))",
                format_expression(expr)?,
                if *negated { " NOT" } else { "" },
                values
            ))
        }
        Expr::Between {
            expr,
            low,
            high,
            negated,
        } => Ok(format!(
            "({}{} BETWEEN {} AND {})",
            format_expression(expr)?,
            if *negated { " NOT" } else { "" },
            format_expression(low)?,
            format_expression(high)?
        )),
        Expr::Not { expr } => Ok(format!("(NOT {})", format_expression(expr)?)),
        Expr::Cast { expr, data_type } => Ok(format!(
            "CAST({} AS {})",
            format_expression(expr)?,
            data_type.type_name()
        )),
        Expr::Exists(_) => Err("EXISTS expressions cannot be stored in rollup aggregates".into()),
        Expr::Function(function) => format_function(function),
    }
}

fn format_function(function: &FunctionCall) -> Result<String, String> {
    let args = function
        .args
        .iter()
        .map(format_expression)
        .collect::<Result<Vec<_>, _>>()?
        .join(", ");
    Ok(format!("{}({args})", function.name.to_ascii_lowercase()))
}

fn format_float(value: f64) -> Result<String, String> {
    if !value.is_finite() {
        return Err("non-finite numeric literals cannot be stored in rollup definitions".into());
    }
    let formatted = value.to_string();
    if formatted.contains(['.', 'e', 'E']) {
        Ok(formatted)
    } else {
        Ok(format!("{formatted}.0"))
    }
}

fn format_binary_operator(operator: &BinaryOp) -> &'static str {
    match operator {
        BinaryOp::Eq => "=",
        BinaryOp::NotEq => "<>",
        BinaryOp::Lt => "<",
        BinaryOp::Lte => "<=",
        BinaryOp::Gt => ">",
        BinaryOp::Gte => ">=",
        BinaryOp::And => "AND",
        BinaryOp::Or => "OR",
        BinaryOp::Add => "+",
        BinaryOp::Sub => "-",
        BinaryOp::Mul => "*",
        BinaryOp::Div => "/",
        BinaryOp::Like => "LIKE",
        BinaryOp::PgvectorCosine => "<=>",
        BinaryOp::PgvectorL2 => "<->",
        BinaryOp::PgvectorDot => "<#>",
    }
}

fn quote_identifier(identifier: &str) -> String {
    crate::sql::ColumnIdentifierPath::parse(identifier).map_or_else(
        |_| format!("\"{}\"", identifier.replace('"', "\"\"")),
        |column| column.canonical_sql(),
    )
}

#[cfg(test)]
mod tests {
    use crate::sql::ast::Expr;

    use super::canonical_aggregate_expression;

    #[test]
    fn should_round_trip_arithmetic_aggregate_expression_as_sql() {
        // Arrange
        let Expr::Function(function) = crate::sql::parser::parse_expression("MAX(amount * 10)")
            .expect("parse source expression")
        else {
            panic!("aggregate expression should parse as a function")
        };

        // Act
        let canonical = canonical_aggregate_expression(&function).expect("format expression");
        let reparsed =
            crate::sql::parser::parse_expression(&canonical).expect("parse canonical SQL");

        // Assert
        assert_eq!(canonical, "max((\"amount\" * 10))");
        assert!(Expr::Function(function).structurally_eq(&reparsed));
    }

    #[test]
    fn should_round_trip_supported_aggregate_expression_forms_as_sql() {
        // Arrange
        let expressions = [
            "MAX(COALESCE(amount, 0) + 2.5)",
            "MAX(CASE WHEN tenant = 'O''Reilly' THEN amount ELSE 0 END)",
            "MAX(CAST(amount AS FLOAT))",
            "MAX(amount BETWEEN 1 AND 5)",
            "MAX(amount IS NOT NULL)",
            "MAX(amount IN (1, 2))",
            "MAX(amount NOT IN (1, 2))",
            "MAX(NOT (amount = 0))",
            "MAX(amount = 1 AND tenant LIKE 'O''Reilly%')",
            "MAX(1.25e2)",
            "MAX(TRUE)",
            "MAX(NULL)",
            "MAX(\"odd\"\"name\")",
            "MAX($1)",
        ];

        // Act
        let round_tripped = expressions
            .iter()
            .map(|source| {
                let Expr::Function(function) =
                    crate::sql::parser::parse_expression(source).expect("parse source expression")
                else {
                    panic!("aggregate expression should parse as a function")
                };
                let canonical =
                    canonical_aggregate_expression(&function).expect("format expression");
                let reparsed =
                    crate::sql::parser::parse_expression(&canonical).expect("parse canonical SQL");
                (source, canonical, Expr::Function(function), reparsed)
            })
            .collect::<Vec<_>>();

        // Assert
        for (source, canonical, original, reparsed) in round_tripped {
            assert!(
                original.structurally_eq(&reparsed),
                "canonical expression {canonical:?} did not preserve source {source:?}: {original:?} != {reparsed:?}"
            );
        }
    }
}
