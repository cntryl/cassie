//! Resolves the `cassie_graph.key_part` overload from its argument's SQL type.
//!
//! "`key_part` is overloaded on the five listed exact input types and rejects
//! NULL; no implicit conversion changes the type tag." The runtime value of an
//! integer does not say whether it is `smallint`, `integer` or `bigint`, so the
//! call is rewritten to the internal `(value, type_tag)` form here, where the
//! argument's static type is known.
use super::coalesce_results::ResultTypes;
use super::{CassieError, DataType, Expr, FunctionCall};
use crate::app::DiagnosticError;
use crate::sql::functions::graph_types::{KEY_PART, KEY_PART_TAGGED};

/// The `type_tag` of an argument type, or `None` when it is not an overload.
fn tag(data_type: &DataType) -> Option<i64> {
    match data_type.flattened() {
        DataType::SmallInt => Some(1),
        DataType::Int => Some(2),
        DataType::BigInt => Some(3),
        DataType::Text => Some(4),
        DataType::Uuid => Some(5),
        _ => None,
    }
}

/// Rewrites a one-argument `key_part` call in place. A call whose argument
/// type is not yet known stays as written and fails at execution.
///
/// # Errors
/// Returns `42883` for an argument type that is not one of the five overloads.
pub(super) fn lower(function: &mut FunctionCall, types: &ResultTypes) -> Result<(), CassieError> {
    if !function.name.eq_ignore_ascii_case(KEY_PART) || function.args.len() != 1 {
        return Ok(());
    }
    let argument = &function.args[0];
    let resolved = match argument {
        // An unknown-typed literal resolves to text, as PostgreSQL prefers
        // the string category; NULL is rejected at execution (`22004`).
        Expr::StringLiteral(_) => Some(DataType::Text),
        Expr::Null => return Ok(()),
        other => types.expression_type(other),
    };
    let Some(data_type) = resolved.filter(|data_type| !matches!(data_type, DataType::Null)) else {
        return Ok(());
    };
    let Some(type_tag) = tag(&data_type) else {
        return Err(DiagnosticError::new(
            "42883",
            format!(
                "function cassie_graph.key_part({}) does not exist",
                data_type.type_name()
            ),
        )
        .with_hint("key_part accepts smallint, integer, bigint, text and uuid.")
        .into());
    };
    function.name = KEY_PART_TAGGED.to_owned();
    function.args.push(Expr::IntegerLiteral(type_tag));
    Ok(())
}
