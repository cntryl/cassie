//! A finite native numeric predicate subset feeds admitted typed selections.
use super::{PreparedSpecs, QueryError};
use crate::catalog::CollectionSchema;
use crate::executor::retained_memory::{add, mul};
use crate::executor::typed_batch::{capability, TypedBatch};
use crate::runtime::QueryExecutionControls;
use crate::sql::ast::{BinaryOp, Expr};
use crate::types::DataType;
use std::mem::size_of;

pub(super) fn prepare(
    expression: Option<&Expr>,
    schema: &CollectionSchema,
    prepared: &mut PreparedSpecs,
) -> Result<bool, QueryError> {
    let Some(expression) = expression else {
        return Ok(true);
    };
    if !collect(expression, schema, prepared)? {
        return Ok(false);
    }
    prepared
        ._memory
        .try_grow(mul(prepared.fields.len(), size_of::<(String, DataType)>())?)?;
    let mut typed_schema = Vec::with_capacity(prepared.fields.len());
    for name in &prepared.fields {
        let Some(field) = schema.fields.iter().find(|field| {
            crate::sql::ColumnIdentifierPath::matches_stored_field(name, &field.name)
        }) else {
            return Ok(false);
        };
        prepared._memory.try_grow(name.len())?;
        typed_schema.push((name.clone(), field.data_type.clone()));
    }
    Ok(capability::expression(expression, &typed_schema, &[])
        == capability::Capability::NativeTyped)
}
fn collect(
    expression: &Expr,
    schema: &CollectionSchema,
    prepared: &mut PreparedSpecs,
) -> Result<bool, QueryError> {
    match expression {
        Expr::Column(name) => {
            let Ok(reference) = crate::sql::ColumnIdentifierPath::parse(name) else {
                return Ok(false);
            };
            if reference.is_qualified() {
                return Ok(false);
            }
            let Some(field) = schema
                .fields
                .iter()
                .find(|field| reference.matches_field_name(&field.name))
            else {
                return Ok(false);
            };
            if !matches!(
                field.data_type,
                DataType::SmallInt | DataType::Int | DataType::BigInt | DataType::Float
            ) {
                return Ok(false);
            }
            if !prepared.fields.contains(name) {
                prepared
                    ._memory
                    .try_grow(add(size_of::<String>(), name.len())?)?;
                prepared.fields.reserve_exact(1);
                prepared.fields.push(name.clone());
            }
            Ok(true)
        }
        Expr::IntegerLiteral(_) | Expr::NumberLiteral(_) | Expr::BoolLiteral(_) | Expr::Null => {
            Ok(true)
        }
        Expr::IsNull { expr, .. } | Expr::Not { expr } => collect(expr, schema, prepared),
        Expr::Binary {
            left,
            op:
                BinaryOp::Eq
                | BinaryOp::NotEq
                | BinaryOp::Lt
                | BinaryOp::Lte
                | BinaryOp::Gt
                | BinaryOp::Gte
                | BinaryOp::And
                | BinaryOp::Or,
            right,
        } => Ok(collect(left, schema, prepared)? && collect(right, schema, prepared)?),
        _ => Ok(false),
    }
}
pub(super) fn apply(
    batch: TypedBatch,
    expression: Option<&Expr>,
    controls: &QueryExecutionControls,
) -> Result<TypedBatch, QueryError> {
    match expression {
        Some(expression) => batch.filter(
            controls,
            expression,
            &[],
            &std::collections::HashMap::new(),
            None,
        ),
        None => Ok(batch),
    }
}
