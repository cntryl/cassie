//! Scalar functions of the reserved `cassie_graph` schema (#1268).
use super::{EvalContext, FunctionCall, QueryError, RowAccess, Value};
use crate::app::{CassieError, DiagnosticError};
use crate::sql::functions::FunctionId;
use crate::types::graph::{GraphDescriptors, KeyPart, Path};
use crate::types::typed::TypedValue;

pub(super) fn failure(state: &'static str, message: impl Into<String>) -> QueryError {
    QueryError::Cassie(CassieError::from(DiagnosticError::new(state, message)))
}

/// The reserved types of the session's database, or `42704` while the schema
/// is not bootstrapped there.
pub(crate) fn descriptors_of(
    session: Option<&crate::app::CassieSession>,
) -> Result<GraphDescriptors, QueryError> {
    let (catalog, binding) = session
        .and_then(crate::app::CassieSession::object_lookup_context)
        .ok_or_else(|| failure("XX000", "cassie_graph functions require a session"))?;
    catalog.graph_descriptors(&binding.database).ok_or_else(|| {
        failure(
            "42704",
            "type cassie_graph.graph_path does not exist: the reserved schema is not bootstrapped",
        )
    })
}

/// Evaluates a `cassie_graph` scalar function call, or `None` for any other.
pub(super) fn evaluate_expression<R: RowAccess + ?Sized>(
    function: &FunctionCall,
    row: &R,
    context: EvalContext<'_>,
) -> Option<Result<Value, QueryError>> {
    let metadata = crate::sql::functions::function(&function.name)?;
    if !matches!(
        metadata.id,
        FunctionId::GraphKeyPart
            | FunctionId::GraphKeyPartTagged
            | FunctionId::GraphVertexRef
            | FunctionId::GraphEdgeRef
            | FunctionId::GraphPathToJsonb
    ) {
        return None;
    }
    let args = function
        .args
        .iter()
        .map(|argument| super::super::evaluate_expr_value_with_context(row, argument, context))
        .collect::<Result<Vec<_>, _>>();
    Some(args.and_then(|args| dispatch(metadata.id, args.as_slice(), context)))
}

fn dispatch(id: FunctionId, args: &[Value], context: EvalContext<'_>) -> Result<Value, QueryError> {
    match id {
        FunctionId::GraphKeyPart => Err(failure(
            "42883",
            "function cassie_graph.key_part could not determine its argument type",
        )
        .with_hint("Cast the argument to smallint, integer, bigint, text or uuid.")),
        FunctionId::GraphKeyPartTagged => key_part(args, context),
        FunctionId::GraphPathToJsonb => path_to_jsonb(args),
        _ => super::graph_refs::evaluate(id, args, context),
    }
}

trait WithHint {
    fn with_hint(self, hint: &str) -> Self;
}

impl WithHint for QueryError {
    fn with_hint(self, hint: &str) -> Self {
        match self {
            Self::Cassie(CassieError::Diagnostic(diagnostic)) => {
                Self::Cassie(CassieError::Diagnostic(Box::new(diagnostic.with_hint(hint))))
            }
            other => other,
        }
    }
}

fn key_part(args: &[Value], context: EvalContext<'_>) -> Result<Value, QueryError> {
    let [value, Value::Int64(tag)] = args else {
        return Err(failure("42883", "cassie_graph.key_part expects one argument"));
    };
    let part = match (tag, value) {
        (_, Value::Null) => {
            return Err(failure(
                "22004",
                "null value not allowed: cassie_graph.key_part rejects NULL",
            ))
        }
        (1, Value::Int64(number)) => i16::try_from(*number).ok().map(KeyPart::SmallInt),
        (2, Value::Int64(number)) => i32::try_from(*number).ok().map(KeyPart::Int),
        (3, Value::Int64(number)) => Some(KeyPart::BigInt(*number)),
        (4, Value::String(text)) => Some(KeyPart::Text(text.clone())),
        (5, Value::String(text)) => crate::types::uuid_text::parse(text).map(KeyPart::Uuid),
        _ => None,
    }
    .ok_or_else(|| failure("42804", "cassie_graph.key_part argument does not match its type"))?;
    let descriptors = descriptors_of(context.session)?;
    part.to_value(&descriptors)
        .map_err(|message| failure("XX000", message))
}

/// The record of a `graph_path` argument; `None` for SQL NULL.
pub(crate) fn path_argument(value: &Value) -> Result<Option<Path>, QueryError> {
    match value {
        Value::Null => Ok(None),
        Value::Typed(TypedValue::Record(record)) => Path::from_record(record)
            .map(Some)
            .map_err(|error| failure("22023", error.message().to_owned())),
        _ => Err(failure(
            "42804",
            "argument must be of type cassie_graph.graph_path",
        )),
    }
}

fn path_to_jsonb(args: &[Value]) -> Result<Value, QueryError> {
    let [argument] = args else {
        return Err(failure("42883", "cassie_graph.path_to_jsonb expects one argument"));
    };
    Ok(path_argument(argument)?.map_or(Value::Null, |path| {
        Value::Json(crate::types::graph::path_to_json(&path))
    }))
}
