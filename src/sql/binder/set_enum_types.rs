//! Set inputs from distinct PostgreSQL type categories cannot share a column.
//! Different named enum types retain their established conversion diagnostic.

use crate::app::{CassieError, DiagnosticError};
use crate::sql::ast::{SelectStatement, SetOperator};
use crate::types::{DataType, EnumRef};

use super::super::{inference, BindingContext, Catalog, CteScope, Schema};

const fn operator_name(operator: SetOperator) -> &'static str {
    match operator {
        SetOperator::Union | SetOperator::UnionAll => "UNION",
        SetOperator::Intersect | SetOperator::IntersectAll => "INTERSECT",
        SetOperator::Except | SetOperator::ExceptAll => "EXCEPT",
    }
}

fn enum_of(data_type: &DataType) -> Option<&EnumRef> {
    match data_type.flattened() {
        DataType::Enum(descriptor) => Some(descriptor),
        DataType::Array(element) => match element.flattened() {
            DataType::Enum(descriptor) => Some(descriptor),
            _ => None,
        },
        _ => None,
    }
}

/// Checks type categories and enum identities known before execution.
///
/// # Errors
///
/// Returns `42804` for incompatible categories or `42846` for distinct enums.
pub(in crate::sql::binder) fn validate(
    select: &SelectStatement,
    catalog: &Catalog,
    context: &BindingContext,
    scope: &CteScope<'_>,
) -> Result<(), CassieError> {
    let Some(set) = &select.set else {
        return Ok(());
    };
    let mut left = select.clone();
    left.set = None;
    let ctes = scope
        .iter()
        .map(|(name, binding)| {
            (
                name.clone(),
                Schema {
                    fields: binding.row_fields.clone(),
                },
            )
        })
        .collect();
    let functions = crate::catalog::function_resolution::functions_for_scope(
        &catalog.list_functions(),
        &context.database,
        &context.search_path,
        context.scopes_database_objects(),
    );
    let schema = |query: &SelectStatement| {
        inference::select_schema(
            query,
            catalog,
            &ctes,
            &functions,
            &[],
            None,
            inference::ProjectionMode::SetOperand,
        )
    };
    let (Ok(left), Ok(right)) = (schema(&left), schema(&set.right)) else {
        return Ok(());
    };
    for (left, right) in left.fields.iter().zip(&right.fields) {
        if let (Some(left_type), Some(right_type)) =
            (builtin(&left.data_type), builtin(&right.data_type))
        {
            if left_type.category != right_type.category {
                return Err(DiagnosticError::new(
                    "42804",
                    format!(
                        "{} types {} and {} cannot be matched",
                        operator_name(set.operator),
                        left_type.display,
                        right_type.display
                    ),
                )
                .into());
            }
        }
        let (Some(left), Some(right)) = (enum_of(&left.data_type), enum_of(&right.data_type))
        else {
            continue;
        };
        if left.oid != right.oid {
            return Err(DiagnosticError::new(
                "42846",
                format!(
                    "{} could not convert type {} to {}",
                    operator_name(set.operator),
                    right.name,
                    left.name
                ),
            )
            .into());
        }
    }
    Ok(())
}

fn builtin(
    data_type: &DataType,
) -> Option<&'static crate::catalog::builtin_identity::BuiltinTypeRow> {
    if *data_type == DataType::Null {
        return None;
    }
    let oid = u32::try_from(data_type.flattened().type_oid()).ok()?;
    crate::catalog::builtin_identity::builtin_identities().type_by_oid(oid)
}
