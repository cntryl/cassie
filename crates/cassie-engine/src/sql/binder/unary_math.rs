//! Select existing primitive unary math signatures before evaluating carriers.
use crate::app::CassieError;
use crate::sql::ast::{AggregateQuantifier, CastIdentity, Expr, FunctionCall, UnaryNumericOp};
use crate::types::{DataType, IntegerWidth};

pub(super) fn resolve(
    expression: &mut Expr,
    data_type: Option<&DataType>,
) -> Result<bool, CassieError> {
    let Expr::UnaryNumeric { op, expr } = expression else {
        return Ok(false);
    };
    let op = *op;
    let root = matches!(op, UnaryNumericOp::SquareRoot | UnaryNumericOp::CubeRoot);
    if !root && op != UnaryNumericOp::Absolute {
        return Ok(false);
    }
    let unknown = matches!(expr.as_ref(), Expr::StringLiteral(_) | Expr::Null);
    // `|/` and `||/` are double precision operators; `numeric` and `real`
    // operands convert implicitly.
    let decimal = root
        && (matches!(
            expr.as_ref(),
            Expr::NumberLiteral(_) | Expr::NumericLiteral(_)
        ) || matches!(data_type, Some(DataType::Extended(_))));
    if !root
        && matches!(
            data_type,
            Some(DataType::Extended(
                crate::types::extended::ExtendedType::Numeric(_)
                    | crate::types::extended::ExtendedType::Real
            ))
        )
    {
        return Ok(true);
    }
    if !unknown && !decimal && crate::sql::functions::has_unselected_numeric(expr) {
        return Err(CassieError::Unsupported(
            "NUMERIC unary math remains pending under #793/#812; select FLOAT8 explicitly".into(),
        ));
    }
    if !unknown
        && !decimal
        && data_type.is_some_and(|kind| {
            IntegerWidth::from_data_type(kind).is_none()
                && !matches!(kind, DataType::Float | DataType::Null)
        })
    {
        return Err(CassieError::Execution(
            "operator does not exist: unary math with non-numeric operand".into(),
        ));
    }
    if unknown || decimal {
        crate::sql::literal_inputs::validate(expr, &DataType::Float, CastIdentity::Scalar)?;
        let operand = std::mem::replace(expr.as_mut(), Expr::Null);
        **expr = Expr::Cast {
            expr: Box::new(operand),
            data_type: DataType::Float,
            source_type: None,
            identity: CastIdentity::Scalar,
        };
        return Ok(true);
    }
    if op == UnaryNumericOp::Absolute {
        if let Some(width) = data_type.and_then(IntegerWidth::from_data_type) {
            let operand = std::mem::replace(expr.as_mut(), Expr::Null);
            let absolute = Expr::Function(Box::new(FunctionCall {
                name: "pg_catalog.abs".into(),
                args: vec![operand],
                quantifier: AggregateQuantifier::Default,
                filter: None,
                order_by: Vec::new(),
            }));
            *expression = Expr::Cast {
                expr: Box::new(absolute),
                data_type: width.data_type(),
                source_type: None,
                identity: CastIdentity::Scalar,
            };
        }
    }
    Ok(true)
}
