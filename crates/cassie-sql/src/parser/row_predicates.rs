//! Recognize row constructors only at a row-predicate boundary.
use super::super::{clauses::strip_parentheses, lexical, schema};
use super::{parse_comparison_expression, parse_expression, split_csv};
use super::{BinaryOp, Expr, SqlError};
use crate::ast::{RowComparison, RowComparisonOp};

pub(super) fn comparison(left: &str, op: BinaryOp, right: &str) -> Result<Expr, SqlError> {
    if matches!(
        op,
        BinaryOp::Eq
            | BinaryOp::NotEq
            | BinaryOp::Lt
            | BinaryOp::Lte
            | BinaryOp::Gt
            | BinaryOp::Gte
    ) && (super::super::operators::ordinary_comparison(left).is_some()
        || super::super::operators::ordinary_comparison(right).is_some())
    {
        return Err(SqlError::syntax(
            "chained comparisons require parentheses".into(),
        ));
    }
    if let Some(subquery) = super::subquery_syntax::comparison(left, &op, right)? {
        return Ok(subquery);
    }
    let constructors = (constructor(left)?, constructor(right)?);
    if let (Some(left), Some(right)) = constructors {
        let op = RowComparisonOp::from_binary(&op).ok_or_else(|| {
            SqlError::unsupported("operator on row constructors is not yet supported".into())
        })?;
        if left.len() != right.len()
            && !left
                .iter()
                .chain(&right)
                .any(|field| matches!(field, Expr::Column(name) if name.ends_with(".*")))
        {
            return Err(SqlError::syntax(
                "unequal number of entries in row expressions".into(),
            ));
        }
        if left.is_empty() && !op.null_safe() {
            return Err(SqlError::unsupported(
                "cannot compare rows of zero length".into(),
            ));
        }
        let expression = Expr::RowCompare(Box::new(RowComparison { left, op, right }));
        super::depth::validate(&expression, 0)?;
        return Ok(expression);
    }
    let (left, left_collation) = split_comparison_collation(left)?;
    let (right, right_collation) = split_comparison_collation(right)?;
    let mut left = parse_comparison_expression(left)?;
    let mut right = parse_comparison_expression(right)?;
    if let Some(collation) = left_collation {
        left = Expr::Collate {
            expr: Box::new(left),
            collation,
        };
    }
    if let Some(collation) = right_collation {
        right = Expr::Collate {
            expr: Box::new(right),
            collation,
        };
    }
    Ok(Expr::Binary {
        left: Box::new(left),
        op,
        right: Box::new(right),
    })
}

pub(super) fn reject_unselected_row_image_operator(raw: &str) -> Result<(), SqlError> {
    for operator in ["*<>", "*<=", "*>=", "*=", "*<", "*>"] {
        let Some((left, right)) = super::super::operators::split_operator(raw, operator, false)
        else {
            continue;
        };
        if constructor(left)?.is_some() && constructor(right)?.is_some() {
            return Err(SqlError::undefined_function(format!(
                "operator does not exist: record {operator} record"
            )));
        }
    }
    Ok(())
}

fn split_comparison_collation(raw: &str) -> Result<(&str, Option<String>), SqlError> {
    let Some((position, end)) =
        super::super::clauses::find_top_level_keyword_span(raw, 0, "collate")
    else {
        return Ok((raw, None));
    };
    let suffix = super::super::lexical::trim_separators(&raw[end..]);
    let name_length = super::collation_name_length(suffix);
    let name = &suffix[..name_length];
    let name_start = raw[end..].find(suffix).unwrap_or(0) + end;
    if name.is_empty() {
        return Err(SqlError::syntax("COLLATE requires a collation name".into()));
    }
    if !raw[name_start + name.len()..].trim().is_empty() {
        return Err(SqlError::syntax(
            "trailing tokens after collation name".into(),
        ));
    }
    Ok((&raw[..position], Some(super::selected_collation(name)?)))
}

pub(super) fn constructor(mut raw: &str) -> Result<Option<Vec<Expr>>, SqlError> {
    loop {
        raw = lexical::trim_separators(raw);
        if super::subquery_syntax::body(raw).is_some() {
            return Ok(None);
        }
        if schema::starts_with_keyword(raw, "row") {
            let Some(inner) = strip_parentheses(lexical::trim_separators(&raw[3..])) else {
                return Ok(None);
            };
            return fields(inner).map(Some);
        }
        let Some(inner) = strip_parentheses(raw) else {
            return Ok(None);
        };
        if split_csv(inner).len() > 1 {
            return fields(inner).map(Some);
        }
        raw = inner;
    }
}

fn fields(raw: &str) -> Result<Vec<Expr>, SqlError> {
    if lexical::trim_separators(raw).is_empty() {
        return Ok(Vec::new());
    }
    let fields = split_csv(raw);
    if fields.len() > 1664 {
        return Err(SqlError::resource_limit(
            "ROW expressions can have at most 1664 entries".into(),
        ));
    }
    fields
        .into_iter()
        .map(|field| {
            let field = lexical::trim_separators(field);
            if let Some(relation) = field.strip_suffix(".*") {
                if !relation.is_empty() && !relation.chars().any(char::is_whitespace) {
                    return Ok(Expr::Column(format!("{relation}.*")));
                }
            }
            parse_expression(field)
        })
        .collect()
}

#[cfg(test)]
mod collation_tests {
    use super::{parse_expression, Expr};

    #[test]
    fn should_keep_both_explicit_collations_on_comparison_operands() {
        // Arrange
        let sql = "left_value COLLATE \"C\" < right_value COLLATE \"POSIX\"";

        // Act
        let expression = parse_expression(sql).expect("comparison parses");

        // Assert
        let Expr::Binary { left, right, .. } = expression else {
            panic!("comparison expression expected");
        };
        assert!(matches!(*left, Expr::Collate { collation, .. } if collation == "C"));
        assert!(matches!(*right, Expr::Collate { collation, .. } if collation == "POSIX"));
    }

    #[test]
    fn should_not_parse_collate_keyword_inside_string_literal_as_collation() {
        // Arrange
        let sql = "'left collate \"C\"' = 'left collate \"C\"'";

        // Act
        let expression = parse_expression(sql).expect("quoted text comparison parses");

        // Assert
        let Expr::Binary { left, right, .. } = expression else {
            panic!("comparison expression expected");
        };
        assert!(matches!(*left, Expr::StringLiteral(value) if value == "left collate \"C\""));
        assert!(matches!(*right, Expr::StringLiteral(value) if value == "left collate \"C\""));
    }
}

#[cfg(test)]
mod comparison_tests {
    use super::{parse_expression, BinaryOp, Expr};
    use crate::parser::{parse_statement, SqlErrorKind};

    #[test]
    fn should_bind_vector_distances_as_operands_of_ordinary_comparisons() {
        // Arrange
        let cases = [
            ("a <-> b < 1", BinaryOp::PgvectorL2, BinaryOp::Lt, true),
            (
                "1 >= a <=> b",
                BinaryOp::PgvectorCosine,
                BinaryOp::Gte,
                false,
            ),
            ("a <#> b = -1", BinaryOp::PgvectorDot, BinaryOp::Eq, true),
        ];

        // Act
        for (sql, distance, comparison, distance_on_left) in cases {
            let expr = parse_expression(sql).expect(sql);
            parse_statement(&format!("SELECT {sql}")).expect(sql);

            // Assert
            let Expr::Binary { left, op, right } = expr else {
                panic!("{sql}")
            };
            assert_eq!(op, comparison, "{sql}");
            let operand = if distance_on_left { left } else { right };
            assert!(
                matches!(*operand, Expr::Binary { op, .. } if op == distance),
                "{sql}"
            );
        }
    }

    #[test]
    fn should_reject_ordinary_comparison_chains_with_vector_operands() {
        // Arrange
        let cases = ["a <-> b < 1 = true", "1 = a <#> b >= 0", "1 < 2 < a <=> b"];

        // Act
        for sql in cases {
            let error = parse_expression(sql).expect_err(sql);
            let statement_error = parse_statement(&format!("SELECT {sql}")).expect_err(sql);

            // Assert
            assert_eq!(error.kind(), SqlErrorKind::Syntax, "{sql}");
            assert_eq!(statement_error.kind(), SqlErrorKind::Syntax, "{sql}");
        }
    }
}
