//! Resolve existing integer operations without adding expression nesting.
use super::{CassieError, DataType, Expr, HashMap};
use crate::catalog::FunctionMeta;
use crate::sql::ast::BinaryOp;
use crate::types::{IntegerWidth, Schema};

pub(super) fn bind(
    expression: &mut Expr,
    functions: &HashMap<String, FunctionMeta>,
    params: &[i32],
    depth: usize,
) -> Result<(), CassieError> {
    if depth > 128 {
        return Err(CassieError::ResourceLimit(
            "VALUES expression nesting exceeds 128 levels".into(),
        ));
    }
    expression.try_visit_children_mut(|child| bind(child, functions, params, depth + 1))?;
    if let Expr::UnaryNumeric { expr, .. } = expression {
        let actual = inferred_type(expr, functions, params);
        return super::super::unary_numeric::resolve(expression, actual.as_ref());
    }
    if let Expr::IntegerNegate { expr, width } = expression {
        let actual = inferred_type(expr, functions, params);
        if actual.as_ref() != Some(&width.data_type()) && actual != Some(DataType::Null) {
            return Err(CassieError::Planner(
                "resolved integer negation does not match its operand type".into(),
            ));
        }
        return Ok(());
    }
    let Expr::Binary { left, op, right } = expression else {
        return Ok(());
    };
    let schema = Schema { fields: Vec::new() };
    let left_type = super::super::inference::infer_expr_type(left, &schema, functions, params);
    let right_type = super::super::inference::infer_expr_type(right, &schema, functions, params);
    super::super::string_operators::qualify(
        left,
        op,
        right,
        left_type.as_ref(),
        right_type.as_ref(),
    );
    let left_type = super::super::inference::infer_expr_type(left, &schema, functions, params);
    let right_type = super::super::inference::infer_expr_type(right, &schema, functions, params);
    if !super::super::integer_operators::resolve(op, left_type.as_ref(), right_type.as_ref())? {
        return Err(CassieError::Unsupported(
            "VALUES arithmetic requires resolved integer operands".into(),
        ));
    }
    Ok(())
}

pub(super) fn inferred_type(
    expression: &Expr,
    functions: &HashMap<String, FunctionMeta>,
    params: &[i32],
) -> Option<DataType> {
    match expression {
        Expr::Binary {
            left,
            op:
                BinaryOp::Add
                | BinaryOp::Sub
                | BinaryOp::Mul
                | BinaryOp::Div
                | BinaryOp::Modulo
                | BinaryOp::Bitwise { .. },
            right,
        } => {
            let left = inferred_type(left, functions, params)?;
            let right = inferred_type(right, functions, params)?;
            let width = match (&left, &right) {
                (DataType::Null, right) => IntegerWidth::from_data_type(right),
                (left, DataType::Null) => IntegerWidth::from_data_type(left),
                (left, right) => IntegerWidth::from_data_type(left)
                    .zip(IntegerWidth::from_data_type(right))
                    .map(|(left, right)| left.max(right)),
            };
            width.map(IntegerWidth::data_type).or_else(|| {
                super::super::inference::infer_expr_type(
                    expression,
                    &Schema { fields: Vec::new() },
                    functions,
                    params,
                )
            })
        }
        Expr::Case {
            branches,
            else_expr,
            ..
        } => branches
            .iter()
            .map(|(_, value)| value)
            .chain(else_expr.as_deref())
            .try_fold(DataType::Null, |common, value| {
                super::super::inference::common_case_type(
                    common,
                    inferred_type(value, functions, params)?,
                )
            }),
        _ => super::super::inference::infer_expr_type(
            expression,
            &Schema { fields: Vec::new() },
            functions,
            params,
        ),
    }
}
