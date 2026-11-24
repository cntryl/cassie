//! Recognize row constructors only at a row-predicate boundary.
use super::super::{clauses::strip_parentheses, lexical, schema};
use super::{parse_comparison_expression, parse_expression, split_csv};
use super::{BinaryOp, Expr, SqlError};
use crate::sql::ast::{RowComparison, RowComparisonOp};

pub(super) fn comparison(left: &str, op: BinaryOp, right: &str) -> Result<Expr, SqlError> {
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
