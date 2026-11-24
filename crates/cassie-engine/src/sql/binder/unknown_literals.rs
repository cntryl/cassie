//! An untyped string literal compared with a typed column takes the column's
//! type, as PostgreSQL resolves `int_column = '7'`.
//!
//! Text, temporal, UUID and BYTEA columns keep their string carrier and are
//! canonicalized by the typed predicate pass that calls this module.
use super::{DataType, Expr};
use crate::sql::ast::{BinaryOp, CastIdentity};

/// Types the literal operands of one comparison, `IN` list or `BETWEEN` node.
pub(super) fn coerce(expr: &mut Expr, field_types: &crate::sql::FieldTypeMap) {
    match expr {
        Expr::Binary { left, op, right } if is_comparison(op) => {
            pair(left, right, field_types);
        }
        Expr::InList { expr, values, .. } => {
            for value in values.iter_mut() {
                pair(expr, value, field_types);
            }
        }
        Expr::Between {
            expr, low, high, ..
        } => {
            pair(expr, low, field_types);
            pair(expr, high, field_types);
        }
        _ => {}
    }
}

const fn is_comparison(op: &BinaryOp) -> bool {
    matches!(
        op,
        BinaryOp::Eq
            | BinaryOp::NotEq
            | BinaryOp::IsDistinctFrom
            | BinaryOp::IsNotDistinctFrom
            | BinaryOp::Lt
            | BinaryOp::Lte
            | BinaryOp::Gt
            | BinaryOp::Gte
    )
}

fn pair(left: &mut Expr, right: &mut Expr, field_types: &crate::sql::FieldTypeMap) {
    let literal_on_right = match (&*left, &*right) {
        (Expr::Column(_), Expr::StringLiteral(_)) => true,
        (Expr::StringLiteral(_), Expr::Column(_)) => false,
        _ => return,
    };
    let (column, literal) = if literal_on_right {
        (left, right)
    } else {
        (right, left)
    };
    let (Expr::Column(column), Expr::StringLiteral(text)) = (&*column, &*literal) else {
        return;
    };
    let Some(typed) = crate::sql::field_type_for_column(field_types, column)
        .and_then(|data_type| typed_literal(text, data_type))
    else {
        return;
    };
    *literal = typed;
}

/// An integer literal stays a plain literal so index bounds still apply; a
/// malformed one keeps its cast and reports `22P02` when it is evaluated.
fn typed_literal(text: &str, data_type: &DataType) -> Option<Expr> {
    let cast = || Expr::Cast {
        expr: Box::new(Expr::StringLiteral(text.to_string())),
        data_type: data_type.clone(),
        source_type: None,
        identity: CastIdentity::Scalar,
    };
    match data_type {
        DataType::SmallInt | DataType::Int | DataType::BigInt => Some(
            text.trim()
                .parse::<i64>()
                .map_or_else(|_| cast(), Expr::IntegerLiteral),
        ),
        DataType::Float
        | DataType::Array(_)
        | DataType::Extended(crate::types::extended::ExtendedType::Numeric(_)) => Some(cast()),
        DataType::Domain(descriptor) => typed_literal(text, &descriptor.base),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{coerce, DataType, Expr};
    use crate::sql::ast::BinaryOp;

    fn comparison(column: &str, literal: &str) -> Expr {
        Expr::Binary {
            left: Box::new(Expr::Column(column.to_string())),
            op: BinaryOp::Eq,
            right: Box::new(Expr::StringLiteral(literal.to_string())),
        }
    }

    #[test]
    fn should_type_a_quoted_literal_by_the_integer_column_it_is_compared_with() {
        // Arrange
        let field_types = crate::sql::FieldTypeMap::from([("i".to_string(), DataType::Int)]);
        let mut expr = comparison("i", "7");

        // Act
        coerce(&mut expr, &field_types);

        // Assert
        assert!(matches!(
            expr,
            Expr::Binary { right, .. } if matches!(*right, Expr::IntegerLiteral(7))
        ));
    }

    #[test]
    fn should_keep_a_quoted_literal_compared_with_a_text_column() {
        // Arrange
        let field_types = crate::sql::FieldTypeMap::from([("t".to_string(), DataType::Text)]);
        let mut expr = comparison("t", "7");

        // Act
        coerce(&mut expr, &field_types);

        // Assert
        assert!(matches!(
            expr,
            Expr::Binary { right, .. } if matches!(*right, Expr::StringLiteral(_))
        ));
    }
}
