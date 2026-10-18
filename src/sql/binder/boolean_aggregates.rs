//! Validate the fixed Boolean aggregate signature with the shared result resolver.

use super::super::coalesce_results::ResultTypes;
use crate::app::CassieError;
use crate::sql::ast::{Expr, FunctionCall};
use crate::sql::functions::{self, FunctionInputSignature};
use crate::types::DataType;

fn is_boolean_aggregate(function: &FunctionCall) -> bool {
    functions::function(&function.name).is_some_and(|metadata| {
        metadata.is_aggregate
            && matches!(
                functions::input_signature(metadata.id),
                Some(FunctionInputSignature::Boolean)
            )
    })
}

pub(super) fn validate(
    function: &mut FunctionCall,
    types: &ResultTypes,
) -> Result<(), CassieError> {
    if !is_boolean_aggregate(function) {
        return Ok(());
    }
    if function.args.len() != 1 {
        return Err(undefined_function(function, types));
    }
    let argument = &mut function.args[0];
    if matches!(argument, Expr::Column(name) if name == "*") {
        return Err(CassieError::Execution(format!(
            "function {}() does not exist",
            function.name.to_ascii_lowercase()
        )));
    }
    if let Expr::StringLiteral(text) = argument {
        let value = crate::types::boolean::parse_text(text).ok_or_else(|| {
            CassieError::Execution(format!("invalid input syntax for type boolean: {text:?}"))
        })?;
        *argument = Expr::BoolLiteral(value);
    }
    match types.expression_type(argument) {
        Some(DataType::Boolean | DataType::Null) | None => Ok(()),
        Some(_) => Err(undefined_function(function, types)),
    }
}

pub(super) fn undefined_function(function: &FunctionCall, types: &ResultTypes) -> CassieError {
    let arguments = function
        .args
        .iter()
        .map(|argument| {
            types
                .expression_type(argument)
                .map_or_else(|| "unknown".to_owned(), |data_type| type_name(&data_type))
        })
        .collect::<Vec<_>>()
        .join(", ");
    CassieError::Execution(format!(
        "function {}({arguments}) does not exist",
        function.name.to_ascii_lowercase()
    ))
}

pub(super) fn type_name(data_type: &DataType) -> String {
    match data_type {
        DataType::Null => "unknown".to_owned(),
        DataType::Int => "integer".to_owned(),
        DataType::Float => "double precision".to_owned(),
        DataType::Char { .. } => "character".to_owned(),
        DataType::Varchar { .. } => "character varying".to_owned(),
        DataType::Array(inner) => format!("{}[]", type_name(inner)),
        other => other.type_name(),
    }
}
