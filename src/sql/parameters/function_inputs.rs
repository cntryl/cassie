//! Input inference for the qualified Lower/COALESCE and primitive SQL-function slice.
//!
//! Catalog signatures are borrowed, never copied with their stored bodies. Unknown
//! finalization is local to an eligible COALESCE, not a statement-wide fallback.

use super::inference::{infer_parameter_type_from_expected_expr, infer_parameter_type_oids_expr};
use super::{ParameterAdmissionError, ParameterInference, UNKNOWN_PARAMETER_TYPE_OID};
use crate::app::CassieError;
use crate::catalog::Catalog;
use crate::sql::ast::{CallProcedureStatement, Expr, FunctionCall};
use crate::sql::functions::{self, FunctionInputSignature, FunctionReturnType};
use crate::sql::{binder, FieldTypeMap};
use crate::types::DataType;

pub(super) fn infer_function_parameter_type_oids(
    function: &FunctionCall,
    field_types: &FieldTypeMap,
    catalog: &Catalog,
    inference: &mut ParameterInference,
) {
    if !inference.check() {
        return;
    }
    for argument in function.expressions() {
        infer_parameter_type_oids_expr(argument, field_types, catalog, inference);
    }
    if let Some(filter) = &function.filter {
        infer_parameter_type_from_expected_expr(filter, &DataType::Boolean, inference);
    }
    if function.args.is_empty() {
        return;
    }
    if !inference.check() {
        return;
    }
    if let Err(error) = infer_inputs(function, field_types, catalog, inference) {
        inference.failure = Some(error);
    }
}

pub(super) fn infer_procedure_parameter_type_oids(
    procedure: &CallProcedureStatement,
    catalog: &Catalog,
    inference: &mut ParameterInference,
) {
    if !inference.check() {
        return;
    }
    let result =
        catalog.with_procedure_signature(&procedure.name, inference.controls, |arguments| {
            let Some(arguments) =
                arguments.filter(|arguments| arguments.len() == procedure.args.len())
            else {
                return;
            };
            for (argument, input) in procedure.args.iter().zip(arguments) {
                if let Some(data_type) = primitive_type(&input.data_type) {
                    infer_parameter_type_from_expected_expr(argument, &data_type, inference);
                }
            }
        });
    if let Err(error) = result {
        inference.failure = Some(admission_error(error));
    }
}

fn infer_inputs(
    function: &FunctionCall,
    field_types: &FieldTypeMap,
    catalog: &Catalog,
    inference: &mut ParameterInference,
) -> Result<(), ParameterAdmissionError> {
    let cast = functions::primitive_casts::candidate(function);
    if let Some(cast) = &cast {
        if matches!(cast.data_type, DataType::Text | DataType::Varchar { .. }) {
            inference.without_context_demands(|inference| {
                infer_parameter_type_from_expected_expr(
                    &function.args[0],
                    &cast.data_type,
                    inference,
                );
            });
            return Ok(());
        }
    }
    let declared = infer_declared_inputs(function, catalog, inference)?;
    if declared || !inference.check() {
        return Ok(());
    }
    let Some(metadata) = functions::function(&function.name)
        .filter(|metadata| metadata.arity.matches(function.args.len()))
    else {
        return Ok(());
    };
    // Both native pg_size_pretty overloads are candidates for an unknown
    // parameter. Only an explicit cast/declaration selects the bigint path.
    if matches!(
        metadata.id,
        functions::FunctionId::PgSizePretty | functions::FunctionId::PgCharToEncoding
    ) {
        return Ok(());
    }
    let data_type = match functions::input_signature(metadata.id) {
        Some(FunctionInputSignature::Positional(oids)) => {
            infer_positional_inputs(function, metadata.id, oids, field_types, catalog, inference)?;
            None
        }
        Some(FunctionInputSignature::RegexpReplace) => {
            infer_regexp_replace_inputs(function, field_types, catalog, inference)?;
            None
        }
        Some(FunctionInputSignature::Text) => Some(DataType::Text),
        Some(FunctionInputSignature::Boolean) => Some(DataType::Boolean),
        Some(FunctionInputSignature::BitwiseInteger) => {
            let input_type = function
                .args
                .first()
                .map(|argument| argument_type(argument, field_types, catalog, inference));
            match input_type.transpose()? {
                Some(ArgumentType::Known(
                    data_type @ (DataType::SmallInt | DataType::Int | DataType::BigInt),
                )) => Some(data_type),
                _ => None,
            }
        }
        Some(FunctionInputSignature::PercentileFraction) => {
            let array_argument = function.args.first().is_some_and(|argument| {
                matches!(
                    argument,
                    Expr::Subquery(subquery)
                        if matches!(subquery.kind, crate::sql::ast::SubqueryKind::Array)
                ) || matches!(
                    argument,
                    Expr::Cast {
                        data_type: DataType::Array(_),
                        ..
                    }
                )
            });
            if !array_argument {
                if let Some(argument) = function.args.first() {
                    infer_parameter_type_from_expected_expr(argument, &DataType::Float, inference);
                }
            }
            None
        }
        Some(FunctionInputSignature::CommonArguments) => {
            common_argument_type(function, field_types, catalog, inference)?
        }
        Some(FunctionInputSignature::HypotheticalSetArguments) => {
            infer_hypothetical_set_inputs(function, field_types, catalog, inference)?
        }
        Some(FunctionInputSignature::OrderedSetOrderKey) | None => None,
    };
    if let Some(data_type) = data_type {
        for argument in &function.args {
            infer_parameter_type_from_expected_expr(argument, &data_type, inference);
        }
    }
    Ok(())
}

fn infer_positional_inputs(
    function: &FunctionCall,
    function_id: functions::FunctionId,
    oids: &'static [i32],
    field_types: &FieldTypeMap,
    catalog: &Catalog,
    inference: &mut ParameterInference,
) -> Result<(), ParameterAdmissionError> {
    let mut integer_substring = false;
    if function_id == functions::FunctionId::Substring {
        for argument in function.args.iter().skip(1) {
            integer_substring |= matches!(
                argument_type(argument, field_types, catalog, inference)?,
                ArgumentType::Known(DataType::SmallInt | DataType::Int)
            );
        }
    }
    let oids = if function
        .name
        .rsplit('.')
        .next()
        .is_some_and(|name| name.eq_ignore_ascii_case("substring"))
        && !integer_substring
    {
        &[25, 25, 25]
    } else {
        oids
    };
    for (argument, oid) in function.args.iter().zip(oids) {
        let data_type = match *oid {
            20 => DataType::BigInt,
            23 => DataType::Int,
            701 => DataType::Float,
            _ => DataType::Text,
        };
        infer_parameter_type_from_expected_expr(argument, &data_type, inference);
    }
    Ok(())
}

fn infer_regexp_replace_inputs(
    function: &FunctionCall,
    field_types: &FieldTypeMap,
    catalog: &Catalog,
    inference: &mut ParameterInference,
) -> Result<(), ParameterAdmissionError> {
    let oids = match function.args.len() {
        3 => &[25, 25, 25][..],
        4 => {
            let start_or_flags = argument_type(&function.args[3], field_types, catalog, inference)?;
            if matches!(
                start_or_flags,
                ArgumentType::Known(DataType::SmallInt | DataType::Int)
            ) {
                &[25, 25, 25, 23][..]
            } else {
                &[25, 25, 25, 25][..]
            }
        }
        5 => &[25, 25, 25, 23, 23][..],
        6 => &[25, 25, 25, 23, 23, 25][..],
        _ => &[],
    };
    for (argument, oid) in function.args.iter().zip(oids) {
        let data_type = if *oid == 23 {
            DataType::Int
        } else {
            DataType::Text
        };
        infer_parameter_type_from_expected_expr(argument, &data_type, inference);
    }
    Ok(())
}

fn infer_hypothetical_set_inputs(
    function: &FunctionCall,
    field_types: &FieldTypeMap,
    catalog: &Catalog,
    inference: &mut ParameterInference,
) -> Result<Option<DataType>, ParameterAdmissionError> {
    if function.args.len() == function.order_by.len() {
        for (argument, order) in function.args.iter().zip(&function.order_by) {
            let ArgumentType::Known(data_type) =
                argument_type(&order.expr, field_types, catalog, inference)?
            else {
                continue;
            };
            infer_parameter_type_from_expected_expr(argument, &data_type, inference);
        }
    }
    Ok(None)
}

enum ArgumentType {
    Known(DataType),
    Unknown,
    Unqualified,
}

fn infer_declared_inputs(
    function: &FunctionCall,
    catalog: &Catalog,
    inference: &mut ParameterInference,
) -> Result<bool, ParameterAdmissionError> {
    let cast = functions::primitive_casts::candidate(function);
    catalog
        .with_function_signature(
            &function.name,
            inference.scope,
            inference.controls,
            |signature| {
                let Some((arguments, _)) = signature else {
                    return false;
                };
                if cast.as_ref().is_some_and(|cast| cast.data_type != DataType::Uuid)
                    && arguments.len() == 1
                    && !matches!(arguments[0].data_type, DataType::Text | DataType::Varchar { .. } | DataType::Char { .. })
                    && matches!(function.args.as_slice(), [Expr::Param(index)] if matches!(inference.oids.get(*index).copied(), None | Some(0 | 705)))
                {
                    return false;
                }
                if arguments.len() == function.args.len() {
                    for (argument, input) in function.args.iter().zip(arguments) {
                        if let Some(data_type) = primitive_type(&input.data_type) {
                            // This visitor only sees Params/Casts; it cannot reenter
                            // the catalog while the signature read guard is held.
                            infer_parameter_type_from_expected_expr(
                                argument, &data_type, inference,
                            );
                        }
                    }
                }
                true
            },
        )
        .map_err(admission_error)
}

fn common_argument_type(
    function: &FunctionCall,
    field_types: &FieldTypeMap,
    catalog: &Catalog,
    inference: &mut ParameterInference,
) -> Result<Option<DataType>, ParameterAdmissionError> {
    let mut common = DataType::Null;
    for argument in &function.args {
        if !inference.check() {
            return Ok(None);
        }
        let data_type = match argument_type(argument, field_types, catalog, inference)? {
            ArgumentType::Known(data_type) => data_type,
            ArgumentType::Unknown => continue,
            ArgumentType::Unqualified => return Ok(None),
        };
        let Some(next) = binder::common_case_type(common, data_type) else {
            return Ok(None);
        };
        common = next;
    }
    Ok(Some(if common == DataType::Null {
        DataType::Text
    } else {
        common
    }))
}

fn argument_type(
    argument: &Expr,
    field_types: &FieldTypeMap,
    catalog: &Catalog,
    inference: &mut ParameterInference,
) -> Result<ArgumentType, ParameterAdmissionError> {
    if !inference.check() {
        return Ok(ArgumentType::Unqualified);
    }
    let known = match argument {
        Expr::Param(index) => {
            let oid = inference
                .oids
                .get(*index)
                .copied()
                .unwrap_or(UNKNOWN_PARAMETER_TYPE_OID);
            if matches!(oid, 0 | UNKNOWN_PARAMETER_TYPE_OID) {
                return Ok(ArgumentType::Unknown);
            }
            u32::try_from(oid)
                .ok()
                .and_then(crate::types::builtin::parameter_builtin_for_oid)
                .and_then(|metadata| primitive_type(&metadata.kind.data_type()))
        }
        Expr::Null => return Ok(ArgumentType::Unknown),
        Expr::Cast { data_type, .. } => primitive_type(data_type),
        Expr::Column(name) => {
            crate::sql::field_type_for_column(field_types, name).and_then(primitive_type)
        }
        Expr::Function(function) => {
            return function_result_type(function, field_types, catalog, inference);
        }
        _ => binder::known_expr_type(argument, field_types)
            .and_then(|data_type| primitive_type(&data_type)),
    };
    Ok(known.map_or(ArgumentType::Unqualified, ArgumentType::Known))
}

fn function_result_type(
    function: &FunctionCall,
    field_types: &FieldTypeMap,
    catalog: &Catalog,
    inference: &mut ParameterInference,
) -> Result<ArgumentType, ParameterAdmissionError> {
    let declared = catalog
        .with_function_signature(
            &function.name,
            inference.scope,
            inference.controls,
            |signature| {
                signature
                    .filter(|(arguments, _)| {
                        !functions::has_distinct_builtin_arity(
                            &function.name,
                            function.args.len(),
                            arguments.len(),
                        )
                    })
                    .map(|(_, result)| primitive_type(result))
            },
        )
        .map_err(admission_error)?;
    if let Some(data_type) = declared {
        return Ok(data_type.map_or(ArgumentType::Unqualified, ArgumentType::Known));
    }
    let Some(metadata) = functions::function(&function.name)
        .filter(|metadata| metadata.arity.matches(function.args.len()))
    else {
        return Ok(ArgumentType::Unqualified);
    };
    match functions::input_signature(metadata.id) {
        Some(FunctionInputSignature::Boolean) => Ok(ArgumentType::Known(DataType::Boolean)),
        Some(FunctionInputSignature::BitwiseInteger) => {
            let Some(argument) = function.args.first() else {
                return Ok(ArgumentType::Unqualified);
            };
            Ok(
                match argument_type(argument, field_types, catalog, inference)? {
                    ArgumentType::Known(
                        data_type @ (DataType::SmallInt | DataType::Int | DataType::BigInt),
                    ) => ArgumentType::Known(data_type),
                    ArgumentType::Unknown | ArgumentType::Known(_) | ArgumentType::Unqualified => {
                        ArgumentType::Unqualified
                    }
                },
            )
        }
        Some(FunctionInputSignature::CommonArguments) => {
            Ok(
                common_argument_type(function, field_types, catalog, inference)?
                    .map_or(ArgumentType::Unqualified, ArgumentType::Known),
            )
        }
        Some(FunctionInputSignature::Text | FunctionInputSignature::RegexpReplace)
            if metadata.return_type == FunctionReturnType::Text =>
        {
            Ok(ArgumentType::Known(DataType::Text))
        }
        Some(FunctionInputSignature::Positional(_)) => {
            Ok(ArgumentType::Known(match metadata.return_type {
                FunctionReturnType::Int => DataType::Int,
                FunctionReturnType::BigInt => DataType::BigInt,
                FunctionReturnType::Boolean => DataType::Boolean,
                FunctionReturnType::Bytea => DataType::Bytea,
                _ => DataType::Text,
            }))
        }
        _ => Ok(ArgumentType::Unqualified),
    }
}

fn primitive_type(data_type: &DataType) -> Option<DataType> {
    matches!(
        data_type,
        DataType::Null
            | DataType::Boolean
            | DataType::SmallInt
            | DataType::Int
            | DataType::BigInt
            | DataType::Float
            | DataType::Text
            | DataType::Char { .. }
            | DataType::Varchar { .. }
    )
    .then(|| data_type.clone())
}

fn admission_error(error: CassieError) -> ParameterAdmissionError {
    match error {
        CassieError::QueryCancelled => ParameterAdmissionError::Cancelled,
        CassieError::DeadlineExceeded => ParameterAdmissionError::Deadline,
        CassieError::ResourceLimit(message) => ParameterAdmissionError::Memory(message),
        other => ParameterAdmissionError::Memory(other.to_string()),
    }
}
