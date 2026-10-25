//! Scalar expression result types and parameter carrier identities.
use super::{
    arithmetic_type, common_case_type, infer_function_return_type, numeric, power_type,
    schema_field_type, types, BinaryOp, DataType, Expr, HashMap, Schema,
};

pub(crate) fn infer_expr_type(
    expr: &Expr,
    source_schema: &Schema,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
    parameter_types: &[i32],
) -> Option<DataType> {
    match expr {
        Expr::Column(name) => schema_field_type(source_schema, name),
        Expr::Case {
            branches,
            else_expr,
            ..
        } => {
            let mut result = DataType::Null;
            for (_, value) in branches {
                result = common_case_type(
                    result,
                    infer_expr_type(value, source_schema, user_functions, parameter_types)?,
                )?;
            }
            if let Some(value) = else_expr {
                result = common_case_type(
                    result,
                    infer_expr_type(value, source_schema, user_functions, parameter_types)?,
                )?;
            }
            Some(result)
        }
        Expr::Cast { data_type, .. } => Some(data_type.clone()),
        Expr::Function(function) => {
            infer_function_return_type(function, source_schema, user_functions, parameter_types)
        }
        Expr::NumericLiteral(_) => Some(numeric::NUMERIC),
        Expr::StringLiteral(_) => Some(DataType::Text),
        Expr::NumberLiteral(_) => Some(DataType::Float),
        Expr::UnaryNumeric { expr, op } => super::super::unary_numeric::result_type(
            *op,
            expr,
            infer_expr_type(expr, source_schema, user_functions, parameter_types),
        ),
        Expr::IntegerNegate { width, .. } => Some(width.data_type()),
        Expr::IntegerLiteral(value) => Some(integer_literal_type(*value)),
        Expr::RowCompare(_)
        | Expr::BoolLiteral(_)
        | Expr::Exists(_)
        | Expr::IsNull { .. }
        | Expr::TruthTest { .. }
        | Expr::InList { .. }
        | Expr::Between { .. }
        | Expr::Like { .. }
        | Expr::Not { .. } => Some(DataType::Boolean),
        Expr::Null => Some(DataType::Null),
        Expr::Subquery(subquery) => subquery.result_type(),
        Expr::Param(index) => parameter_types
            .get(*index)
            .copied()
            .and_then(data_type_for_parameter_oid)
            .or(Some(DataType::Null)),
        Expr::Binary { left, op, right } => infer_binary_type(
            left,
            op,
            right,
            source_schema,
            user_functions,
            parameter_types,
        ),
    }
}

fn infer_binary_type(
    left: &Expr,
    op: &BinaryOp,
    right: &Expr,
    source_schema: &Schema,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
    parameter_types: &[i32],
) -> Option<DataType> {
    super::super::extended_inference::binary_type(
        left,
        op,
        right,
        source_schema,
        user_functions,
        parameter_types,
    )
    .or_else(|| match op {
        BinaryOp::Typed { .. }
        | BinaryOp::Quantified { .. }
        | BinaryOp::JsonbCompare { .. }
        | BinaryOp::And
        | BinaryOp::Or
        | BinaryOp::Eq
        | BinaryOp::NotEq
        | BinaryOp::IsDistinctFrom
        | BinaryOp::IsNotDistinctFrom
        | BinaryOp::Lt
        | BinaryOp::Lte
        | BinaryOp::Gt
        | BinaryOp::Gte
        | BinaryOp::Like
        | BinaryOp::JsonPathExists
        | BinaryOp::JsonPathMatch => Some(DataType::Boolean),
        BinaryOp::JsonbKeyExists | BinaryOp::JsonbKeyExistsAny | BinaryOp::JsonbKeyExistsAll => {
            Some(DataType::Boolean)
        }
        BinaryOp::JsonbDeletePath => {
            infer_expr_type(left, source_schema, user_functions, parameter_types)
                .filter(DataType::is_json)
                .or(Some(DataType::Jsonb))
        }
        BinaryOp::JsonExtract { as_text, .. } => Some(if *as_text {
            DataType::Text
        } else {
            infer_expr_type(left, source_schema, user_functions, parameter_types)
                .filter(DataType::is_json)
                .unwrap_or(DataType::Jsonb)
        }),
        BinaryOp::Sub
            if infer_expr_type(left, source_schema, user_functions, parameter_types)
                .is_some_and(|kind| kind.is_json()) =>
        {
            infer_expr_type(left, source_schema, user_functions, parameter_types)
        }
        BinaryOp::Power => Some(power_type(
            left,
            right,
            source_schema,
            user_functions,
            parameter_types,
        )),
        BinaryOp::Concat => types::infer_concatenation_type(
            left,
            right,
            source_schema,
            user_functions,
            parameter_types,
        ),
        BinaryOp::ByteaConcat => Some(DataType::Bytea),
        BinaryOp::Bitwise { operation } if operation.is_shift() => {
            infer_expr_type(left, source_schema, user_functions, parameter_types)
        }
        BinaryOp::Integer { width, .. } => Some(width.data_type()),
        BinaryOp::Add
        | BinaryOp::Sub
        | BinaryOp::Mul
        | BinaryOp::Div
        | BinaryOp::Modulo
        | BinaryOp::Bitwise { .. } => types::temporal_arithmetic_type(
            (left, op, right),
            source_schema,
            user_functions,
            parameter_types,
        )
        .or_else(|| {
            Some(arithmetic_type(
                left,
                right,
                matches!(op, BinaryOp::Modulo),
                source_schema,
                user_functions,
                parameter_types,
            ))
        }),
        BinaryOp::PgvectorCosine | BinaryOp::PgvectorL2 | BinaryOp::PgvectorDot => {
            Some(DataType::Float)
        }
    })
}

/// PostgreSQL types an integer literal as `int4` when it fits and `int8`
/// otherwise.
pub(crate) fn integer_literal_type(value: i64) -> DataType {
    if i32::try_from(value).is_ok() {
        DataType::Int
    } else {
        DataType::BigInt
    }
}

/// Returns the known type of an expression without catalog access; expressions
/// needing a resolved source schema return `None`.
pub(crate) fn known_expr_type(
    expr: &Expr,
    field_types: &crate::sql::FieldTypeMap,
) -> Option<DataType> {
    match expr {
        Expr::Column(name) => crate::sql::field_type_for_column(field_types, name).cloned(),
        Expr::Cast { data_type, .. } => Some(data_type.clone()),
        Expr::Subquery(subquery) => subquery.result_type(),
        Expr::StringLiteral(_) => Some(DataType::Text),
        Expr::NumberLiteral(_) => Some(DataType::Float),
        Expr::UnaryNumeric { expr, op } => {
            super::super::unary_numeric::result_type(*op, expr, known_expr_type(expr, field_types))
        }
        Expr::IntegerNegate { width, .. } => Some(width.data_type()),
        Expr::IntegerLiteral(value) => Some(integer_literal_type(*value)),
        Expr::RowCompare(_) | Expr::BoolLiteral(_) => Some(DataType::Boolean),
        Expr::Null => Some(DataType::Null),
        _ => None,
    }
}

pub(crate) fn data_type_for_parameter_oid(oid: i32) -> Option<DataType> {
    if let Some(element_oid) = u32::try_from(oid)
        .ok()
        .and_then(crate::catalog::builtin_identity::builtin_array_element_oid)
    {
        let element_oid = i32::try_from(element_oid).ok()?;
        let element_type = scalar_data_type_for_parameter_oid(element_oid)?;
        if element_type == DataType::Name {
            return None;
        }
        return Some(DataType::Array(Box::new(element_type)));
    }

    scalar_data_type_for_parameter_oid(oid)
}

fn scalar_data_type_for_parameter_oid(oid: i32) -> Option<DataType> {
    if let Some(metadata) = u32::try_from(oid)
        .ok()
        .and_then(crate::types::builtin::parameter_builtin_for_oid)
    {
        return Some(metadata.kind.data_type());
    }
    crate::types::schema::vector_dimensions_for_oid(i64::from(oid)).map(DataType::Vector)
}
