//! Qualify the fixed BYTEA signatures while unknown literal origin is available.
use super::coalesce_results::ResultTypes;
use crate::app::CassieError;
use crate::sql::ast::{CastIdentity, Expr, FunctionCall};
use crate::sql::functions::{self, FunctionInputSignature};
use crate::types::DataType;

pub(super) fn qualify(function: &mut FunctionCall, types: &ResultTypes) -> Result<(), CassieError> {
    if types
        .declared_function(&function.name)
        .is_some_and(|metadata| {
            !functions::has_distinct_builtin_arity(
                &function.name,
                function.args.len(),
                metadata.args.len(),
            )
        })
    {
        return Ok(());
    }
    let Some(metadata) = functions::function(&function.name).filter(|metadata| {
        functions::is_bytea_function(metadata.id) || metadata.id == functions::FunctionId::Lower
    }) else {
        return Ok(());
    };
    let oids = match functions::input_signature(metadata.id) {
        Some(FunctionInputSignature::Positional(oids)) => oids,
        Some(FunctionInputSignature::Text) => &[25],
        _ => return Ok(()),
    };
    if !metadata.arity.matches(function.args.len())
        || function.args.iter().zip(oids).any(|(argument, oid)| {
            !matches!(argument, Expr::StringLiteral(_) | Expr::Null)
                && types
                    .expression_type(argument)
                    .is_some_and(|kind| !accepts(*oid, &kind))
        })
    {
        return Err(undefined(function, types));
    }
    let first_unknown = function
        .args
        .first()
        .is_some_and(|argument| matches!(argument, Expr::StringLiteral(_) | Expr::Null));
    let index_oid = function
        .args
        .get(1)
        .and_then(|argument| types.expression_type(argument))
        .map_or(705, |kind| match kind {
            DataType::BigInt => 20,
            DataType::SmallInt => 21,
            DataType::Int => 23,
            _ => 705,
        });
    match functions::bit_input_selection(metadata.id,first_unknown,index_oid) {
        functions::BitInputSelection::Bytea => {},
        functions::BitInputSelection::Ambiguous => return Err(CassieError::AmbiguousFunction(signature_message(function,types,") is not unique"))),
        functions::BitInputSelection::PendingBit => return Err(CassieError::Unsupported("BIT overload input remains pending under #793/#816; use an explicitly typed BYTEA argument".into())),
    }
    for (argument, oid) in function.args.iter_mut().zip(oids) {
        let target = match oid {
            17 => DataType::Bytea,
            23 => DataType::Int,
            20 => DataType::BigInt,
            _ => DataType::Text,
        };
        if matches!(argument, Expr::StringLiteral(_)) {
            crate::sql::literal_inputs::validate(argument, &target, CastIdentity::Scalar)?;
            *argument = Expr::Cast {
                expr: Box::new(std::mem::replace(argument, Expr::Null)),
                data_type: target,
                source_type: None,
                identity: CastIdentity::Scalar,
            };
        } else if *oid == 25
            && matches!(types.expression_type(argument), Some(DataType::Char { .. }))
        {
            let source_type = types.expression_type(argument);
            *argument = Expr::Cast {
                expr: Box::new(std::mem::replace(argument, Expr::Null)),
                data_type: target,
                source_type,
                identity: CastIdentity::Scalar,
            };
        }
    }
    Ok(())
}
fn accepts(oid: i32, kind: &DataType) -> bool {
    matches!(kind, DataType::Null)
        || match oid {
            17 => matches!(kind, DataType::Bytea),
            25 => matches!(
                kind,
                DataType::Text | DataType::Char { .. } | DataType::Varchar { .. }
            ),
            20 => matches!(kind, DataType::SmallInt | DataType::Int | DataType::BigInt),
            23 => matches!(kind, DataType::SmallInt | DataType::Int),
            _ => false,
        }
}
fn undefined(function: &FunctionCall, types: &ResultTypes) -> CassieError {
    CassieError::Execution(signature_message(function, types, ") does not exist"))
}
fn signature_message(function: &FunctionCall, types: &ResultTypes, suffix: &str) -> String {
    let arguments = function
        .args
        .iter()
        .map(|argument| {
            if matches!(argument, Expr::StringLiteral(_) | Expr::Null) {
                "unknown".into()
            } else {
                types
                    .expression_type(argument)
                    .map_or_else(|| "unknown".into(), |kind| type_name(&kind))
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "function {}({arguments}{suffix}",
        function.name.to_ascii_lowercase()
    )
}

fn type_name(kind: &DataType) -> String {
    match kind {
        DataType::Int => "integer".into(),
        DataType::Float => "double precision".into(),
        DataType::Char { .. } => "character".into(),
        DataType::Varchar { .. } => "character varying".into(),
        other => other.type_name(),
    }
}
