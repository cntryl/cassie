//! `enum_first`, `enum_last` and `enum_range` (#827).
//!
//! The argument is `anyenum`; it is usually `NULL::type`, so the enum type
//! comes from the argument's declared type even when its value is NULL.

use super::{EvalContext, FunctionCall, QueryError, RowAccess, Value};
use crate::app::{CassieError, DiagnosticError};
use crate::sql::ast::Expr;
use crate::sql::functions::FunctionId;
use crate::types::enum_type::{EnumInputError, EnumMember};
use crate::types::typed::TypedValue;
use crate::types::{ArrayDimension, DataType, EnumRef, EnumValue, SqlArray};

fn failure(state: &'static str, message: String) -> QueryError {
    QueryError::Cassie(CassieError::from(DiagnosticError::new(state, message)))
}

/// One evaluated `anyenum` argument: its declared type, when known, and its
/// value.
struct Argument {
    descriptor: Option<EnumRef>,
    value: Option<EnumValue>,
    /// An unknown-typed literal, resolved against the other argument's enum.
    label: Option<String>,
}

fn declared_enum(expr: &Expr) -> Option<EnumRef> {
    match expr {
        Expr::Cast { data_type, .. } => match data_type.flattened() {
            DataType::Enum(descriptor) => Some(descriptor.clone()),
            _ => None,
        },
        _ => None,
    }
}

/// The declared enum of a column argument. A NULL column value carries no
/// descriptor, so the type comes from the row's column metadata.
fn column_enum<R: RowAccess + ?Sized>(expr: &Expr, row: &R) -> Option<EnumRef> {
    let Expr::Column(name) = expr else {
        return None;
    };
    match row.column_type(name)?.flattened() {
        DataType::Enum(descriptor) => Some(descriptor.clone()),
        _ => None,
    }
}

fn argument<R: RowAccess + ?Sized>(
    expr: &Expr,
    row: &R,
    context: EvalContext<'_>,
) -> Result<Argument, QueryError> {
    let value = match super::super::evaluate_expr_value_with_context(row, expr, context)? {
        Value::Typed(TypedValue::Enum(value)) => Some(value),
        Value::Null => None,
        Value::String(label) if matches!(expr, Expr::StringLiteral(_)) => {
            return Ok(Argument {
                descriptor: None,
                value: None,
                label: Some(label),
            });
        }
        _ => {
            return Err(failure(
                "42804",
                "enum support function expects an enum argument".to_owned(),
            ))
        }
    };
    let descriptor = declared_enum(expr)
        .or_else(|| value.as_ref().map(|v| v.descriptor().clone()))
        .or_else(|| column_enum(expr, row));
    Ok(Argument {
        descriptor,
        value,
        label: None,
    })
}

fn input_failure(error: &EnumInputError) -> QueryError {
    QueryError::Cassie(CassieError::from(
        crate::executor::filter::enum_cast::input_diagnostic(error),
    ))
}

/// A returned member. PostgreSQL rejects returning a label added in the open
/// transaction to a type that existed before it (`55P04`), as on input.
fn output(descriptor: &EnumRef, member: &EnumMember) -> Result<Value, QueryError> {
    if member.uncommitted {
        return Err(input_failure(&EnumInputError::UncommittedLabel {
            type_name: descriptor.name.clone(),
            label: member.label.clone(),
        }));
    }
    EnumValue::new(descriptor.clone(), member.member_id)
        .map(|value| Value::Typed(TypedValue::Enum(value)))
        .ok_or_else(|| failure("XX000", format!("enum {} lost a member", descriptor.name)))
}

fn boundary(descriptor: &EnumRef, first: bool) -> Result<Value, QueryError> {
    let member = if first {
        descriptor.members.first()
    } else {
        descriptor.members.last()
    };
    let member = member.ok_or_else(|| {
        failure(
            "55000",
            format!("enum {} contains no values", descriptor.name),
        )
    })?;
    output(descriptor, member)
}

fn position(descriptor: &EnumRef, value: &EnumValue) -> Option<usize> {
    descriptor
        .members
        .iter()
        .position(|member| member.member_id == value.member_id())
}

/// `enum_range(lower, upper)`: the members from `lower` to `upper` in sort
/// order; a NULL bound is open-ended, and `lower > upper` is empty.
fn range(
    descriptor: &EnumRef,
    lower: Option<&EnumValue>,
    upper: Option<&EnumValue>,
) -> Result<Value, QueryError> {
    let start = lower.map_or(Some(0), |lower| position(descriptor, lower));
    let end = upper.map_or_else(
        || descriptor.members.len().checked_sub(1),
        |upper| position(descriptor, upper),
    );
    let (Some(start), Some(end)) = (start, end) else {
        return Ok(Value::Array(SqlArray::empty()));
    };
    if start > end {
        return Ok(Value::Array(SqlArray::empty()));
    }
    let elements = descriptor.members[start..=end]
        .iter()
        .map(|member| output(descriptor, member))
        .collect::<Result<Vec<_>, _>>()?;
    let length = i32::try_from(elements.len())
        .map_err(|_| failure("54000", "enum range exceeds the array limit".to_owned()))?;
    SqlArray::new(
        vec![ArrayDimension {
            length,
            lower_bound: 1,
        }],
        elements,
    )
    .map(Value::Array)
    .map_err(|message| failure("XX000", message))
}

/// Evaluates an enum support function call, or `None` for any other function.
pub(super) fn evaluate_expression<R: RowAccess + ?Sized>(
    function: &FunctionCall,
    row: &R,
    context: EvalContext<'_>,
) -> Option<Result<Value, QueryError>> {
    let metadata = crate::sql::functions::function(&function.name)?;
    if !matches!(
        metadata.id,
        FunctionId::EnumFirst | FunctionId::EnumLast | FunctionId::EnumRange
    ) {
        return None;
    }
    Some(evaluate(metadata.id, function, row, context))
}

fn evaluate<R: RowAccess + ?Sized>(
    id: FunctionId,
    function: &FunctionCall,
    row: &R,
    context: EvalContext<'_>,
) -> Result<Value, QueryError> {
    let mut arguments = function
        .args
        .iter()
        .map(|expr| argument(expr, row, context))
        .collect::<Result<Vec<_>, _>>()?;
    let descriptor = arguments
        .iter()
        .find_map(|argument| argument.descriptor.clone())
        .ok_or_else(|| {
            failure(
                "42P18",
                "could not determine polymorphic type because input has type unknown".to_owned(),
            )
        })?;
    if let Some(other) = arguments
        .iter()
        .filter_map(|argument| argument.descriptor.as_ref())
        .find(|other| other.oid != descriptor.oid)
    {
        // `anyenum` arguments must be one enum type; PostgreSQL finds no
        // function for a mixed pair.
        return Err(failure(
            "42883",
            format!(
                "function {}({}, {}) does not exist",
                function.name.to_ascii_lowercase(),
                descriptor.name,
                other.name
            ),
        ));
    }
    for argument in &mut arguments {
        if let Some(label) = argument.label.take() {
            let value = EnumValue::parse_label(&descriptor, &label)
                .map_err(|error| input_failure(&error))?;
            argument.value = Some(value);
        }
    }
    match id {
        FunctionId::EnumFirst => boundary(&descriptor, true),
        FunctionId::EnumLast => boundary(&descriptor, false),
        _ => {
            let bound = |index: usize| arguments.get(index).and_then(|a| a.value.as_ref());
            if arguments.len() == 1 {
                range(&descriptor, None, None)
            } else {
                range(&descriptor, bound(0), bound(1))
            }
        }
    }
}

#[cfg(test)]
#[path = "enum_functions_tests.rs"]
mod tests;
