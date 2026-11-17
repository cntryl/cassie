//! Resolves the `cassie_graph.key_part` overload from its argument's SQL type.
//!
//! "`key_part` is overloaded on the five listed exact input types and rejects
//! NULL; no implicit conversion changes the type tag." The runtime value of an
//! integer does not say whether it is `smallint`, `integer` or `bigint`, so the
//! call is rewritten to the internal `(value, type_tag)` form here, where the
//! argument's static type is known. The constructors are cast to their reserved
//! type so the result column carries it on the wire.
use super::coalesce_results::ResultTypes;
use super::{CassieError, DataType, Expr, FunctionCall};
use crate::app::DiagnosticError;
use crate::sql::ast::CastIdentity;
use crate::sql::functions::graph_types::{
    EDGE_REF, EDGE_REF_TYPED, KEY_PART, KEY_PART_TAGGED, VERTEX_REF, VERTEX_REF_TYPED,
};
use crate::types::composite::CompositeRef;

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

/// A call wrapped in the cast that gives its result the reserved type.
fn typed(function: FunctionCall, target: &CompositeRef) -> Expr {
    Expr::Cast {
        expr: Box::new(Expr::Function(Box::new(function))),
        data_type: DataType::Composite(target.clone()),
        source_type: None,
        identity: CastIdentity::Scalar,
    }
}

/// Validate SQL array identity after nested overloads have been lowered. Runtime record
/// text is a legitimate carrier only after binding proved graph_key_part[].
pub(super) fn validate_reference_arguments(
    function: &FunctionCall,
    types: &ResultTypes,
) -> Result<(), CassieError> {
    let Some(graph) = types.graph_types() else {
        return Ok(());
    };
    let name = function.name.to_ascii_lowercase();
    if !matches!(
        name.as_str(),
        VERTEX_REF | EDGE_REF | VERTEX_REF_TYPED | EDGE_REF_TYPED
    ) {
        return Ok(());
    }
    let Some(argument) = function.args.get(2) else {
        return Ok(());
    };
    let Some(data_type) = types.expression_type(argument) else {
        return Ok(());
    };
    if matches!(data_type, DataType::Null)
        || matches!(data_type.flattened(), DataType::Array(element)
            if matches!(element.flattened(), DataType::Composite(descriptor) if descriptor.oid == graph.key_part.oid))
    {
        return Ok(());
    }
    Err(DiagnosticError::new(
        "42804",
        "key parts must have type cassie_graph.graph_key_part[]",
    )
    .into())
}

/// Lowers a graph constructor call: `key_part` resolves its overload and the
/// reference constructors only gain their result type. A call whose argument
/// type is not yet known stays as written and fails at execution.
///
/// # Errors
/// Returns `42883` for an argument type that is not one of the five overloads.
pub(super) fn lower(
    function: &FunctionCall,
    types: &ResultTypes,
) -> Result<Option<Expr>, CassieError> {
    let Some(graph) = types.graph_types() else {
        return Ok(None);
    };
    let name = function.name.to_ascii_lowercase();
    if name == VERTEX_REF || name == EDGE_REF {
        let mut lowered = function.clone();
        let typed_name = if name == VERTEX_REF {
            VERTEX_REF_TYPED
        } else {
            EDGE_REF_TYPED
        };
        typed_name.clone_into(&mut lowered.name);
        return Ok(Some(typed(lowered, &graph.element_ref)));
    }
    if name != KEY_PART || function.args.len() != 1 {
        return Ok(None);
    }
    let argument = &function.args[0];
    let resolved = match argument {
        // An unknown-typed literal resolves to text, as PostgreSQL prefers
        // the string category; NULL is rejected at execution (`22004`).
        Expr::StringLiteral(_) => Some(DataType::Text),
        Expr::Null => return Ok(None),
        other => types.expression_type(other),
    };
    let Some(data_type) = resolved.filter(|data_type| !matches!(data_type, DataType::Null)) else {
        return Ok(None);
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
    let mut tagged = function.clone();
    KEY_PART_TAGGED.clone_into(&mut tagged.name);
    tagged.args.push(Expr::IntegerLiteral(type_tag));
    Ok(Some(typed(tagged, &graph.key_part)))
}
