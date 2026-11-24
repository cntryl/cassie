//! Scalar signature introspection over Cassie's pinned PostgreSQL built-in identities.

use super::{QueryError, Value};

pub(super) fn evaluate(name: &str, args: &[Value]) -> Option<Result<Value, QueryError>> {
    let local_name = name.strip_prefix("pg_catalog.").unwrap_or(name);
    let format = match local_name {
        "pg_get_function_arguments" | "pg_get_function_identity_arguments" => {
            SignatureFormat::Arguments
        }
        "pg_get_function_result" => SignatureFormat::Result,
        _ => return None,
    };

    Some(evaluate_signature(name, args, format))
}

#[derive(Clone, Copy)]
enum SignatureFormat {
    Arguments,
    Result,
}

fn evaluate_signature(
    name: &str,
    args: &[Value],
    format: SignatureFormat,
) -> Result<Value, QueryError> {
    if args.len() != 1 {
        return Err(QueryError::General(format!("{name} requires 1 arg")));
    }
    let Value::Int64(oid) = &args[0] else {
        return Ok(Value::Null);
    };
    let Ok(oid) = u32::try_from(*oid) else {
        return Ok(Value::Null);
    };
    let identities = crate::catalog::builtin_identity::builtin_identities();
    let Some(function) = identities.proc_by_oid(oid) else {
        return Ok(Value::Null);
    };

    let display_type = |type_oid| {
        identities
            .type_by_oid(type_oid)
            .map(|metadata| metadata.display.clone())
    };
    let result = match format {
        SignatureFormat::Arguments => function
            .argument_types
            .iter()
            .map(|type_oid| display_type(*type_oid))
            .collect::<Option<Vec<_>>>()
            .map(|types| types.join(", ")),
        SignatureFormat::Result => display_type(function.return_type).map(|type_name| {
            if function.returns_set {
                format!("SETOF {type_name}")
            } else {
                type_name
            }
        }),
    };
    Ok(result.map_or(Value::Null, Value::String))
}
