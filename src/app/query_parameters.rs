//! Canonicalizes bound string parameters by their inferred SQL type.
//!
//! Inline `UUID`, `BYTEA`, `DATE`, `TIME` and `TIMESTAMP` literals are
//! canonicalized against their column type by the binder and on write. A
//! bound `Value::String` parameter must match the same rows, so each one
//! whose type the statement implies is rewritten to the same canonical text.

use super::CassieError;
use crate::catalog::Catalog;
use crate::sql::ast::ParsedStatement;
use crate::types::{DataType, Value};

pub(super) fn canonicalize_string_parameters(
    parsed: &ParsedStatement,
    catalog: &Catalog,
    params: &mut [Value],
) {
    if !params.iter().any(|value| matches!(value, Value::String(_))) {
        return;
    }
    let oids = crate::sql::parameter_type_oids_with_catalog(parsed, &[], catalog);
    for (param, oid) in params.iter_mut().zip(oids) {
        let Value::String(text) = param else {
            continue;
        };
        let canonical = match crate::sql::binder::parameter_data_type_for_oid(oid) {
            Some(DataType::Uuid) => uuid::Uuid::parse_str(text)
                .ok()
                .map(|uuid| uuid.to_string()),
            Some(DataType::Bytea) => crate::midge::row_blob::canonical_bytea_text(text).ok(),
            Some(DataType::Date) => crate::types::temporal::canonical_date(text).ok(),
            Some(DataType::Time) => crate::types::temporal::canonical_time(text).ok(),
            Some(DataType::Timestamp) => crate::types::temporal::canonical_timestamp(text).ok(),
            _ => None,
        };
        if let Some(canonical) = canonical {
            *text = canonical;
        }
    }
}

pub(super) fn parameter_type_oids(params: &[Value]) -> Vec<i32> {
    params
        .iter()
        .map(|value| match value {
            Value::Null => 0,
            Value::Bool(_) => 16,
            Value::Int64(_) => 20,
            Value::Float64(_) => 701,
            Value::String(_) => 25,
            Value::Vector(vector) => {
                i32::try_from(DataType::Vector(vector.dimension()).type_oid()).unwrap_or(0)
            }
            Value::Json(_) => 114,
        })
        .collect()
}

pub(super) fn effective_parameter_type_oids(params: &[Value], declared: &[i32]) -> Vec<i32> {
    let mut oids = parameter_type_oids(params);
    for (oid, declared) in oids.iter_mut().zip(declared) {
        if crate::sql::binder::parameter_data_type_for_oid(*declared).is_some() {
            *oid = *declared;
        }
    }
    oids
}

pub(super) fn parameter_shape_for_oids(oids: &[i32]) -> Vec<crate::runtime::ParameterShape> {
    use crate::runtime::ParameterShape;

    oids.iter()
        .map(
            |oid| match crate::sql::binder::parameter_data_type_for_oid(*oid) {
                Some(DataType::Boolean) => ParameterShape::Bool,
                Some(DataType::SmallInt | DataType::Int | DataType::BigInt) => {
                    ParameterShape::Int64
                }
                Some(DataType::Float) => ParameterShape::Float64,
                Some(
                    DataType::Text
                    | DataType::Char { .. }
                    | DataType::Varchar { .. }
                    | DataType::Uuid
                    | DataType::Bytea
                    | DataType::Date
                    | DataType::Time
                    | DataType::Timestamp,
                ) => ParameterShape::String,
                Some(DataType::Vector(dimensions)) => ParameterShape::Vector(dimensions),
                Some(DataType::Json | DataType::Array(_)) => ParameterShape::Json,
                Some(DataType::Null) | None => ParameterShape::Null,
            },
        )
        .collect()
}

pub(super) fn validate_plan_parameters(
    plan: &crate::planner::logical::LogicalPlan,
    catalog: &Catalog,
    context: &crate::sql::binder::BindingContext,
    parameter_types: &[i32],
) -> Result<(), CassieError> {
    if parameter_types.is_empty() {
        return Ok(());
    }
    if crate::executor::plan_uses_function_including_views(plan, "coalesce", catalog)
        || matches!(
            plan.command,
            Some(
                crate::planner::logical::LogicalCommand::Insert(_)
                    | crate::planner::logical::LogicalCommand::Update(_)
                    | crate::planner::logical::LogicalCommand::Delete(_)
            )
        )
    {
        // Keep the existing contextual UUID, BYTEA and temporal parameter validation.
        crate::sql::binder::validate_coalesce_plan(plan, catalog, context, parameter_types, true)?;
    }
    crate::sql::binder::validate_boolean_parameter_plan(plan, catalog, context, parameter_types)
}
