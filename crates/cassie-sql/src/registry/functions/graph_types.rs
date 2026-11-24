//! The scalar `cassie_graph` API (#1268). Every name is registered with its
//! schema, so only the qualified spelling `cassie_graph.<name>` resolves; the
//! unqualified name and any `search_path` entry find nothing.
//!
//! `key_part` is overloaded on the exact SQL type of its argument, which the
//! evaluator cannot recover from a runtime value. The binder lowers a call to
//! the internal two-argument form carrying the type tag.

use super::{FunctionArity, FunctionId, FunctionReturnType, ScalarFunction};
use crate::types::DataType;

/// The user-visible `cassie_graph.key_part(value)`.
pub const KEY_PART: &str = "cassie_graph.key_part";
/// The lowered `(value, type_tag)` form; the double underscore keeps it out
/// of the documented API.
pub const KEY_PART_TAGGED: &str = "cassie_graph.__key_part_tagged";

/// The user-visible reference constructors and their lowered forms, which the
/// binder wraps in a cast to the reserved type (the double underscore keeps
/// them out of the documented API and makes the lowering idempotent).
pub const VERTEX_REF: &str = "cassie_graph.vertex_ref";
pub const EDGE_REF: &str = "cassie_graph.edge_ref";
pub const VERTEX_REF_TYPED: &str = "cassie_graph.__vertex_ref";
pub const EDGE_REF_TYPED: &str = "cassie_graph.__edge_ref";

pub(super) const FUNCTIONS: &[ScalarFunction] = &[
    ScalarFunction {
        id: FunctionId::GraphKeyPart,
        name: KEY_PART,
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Unknown,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::GraphKeyPartTagged,
        name: KEY_PART_TAGGED,
        arity: FunctionArity::Exact(2),
        return_type: FunctionReturnType::Unknown,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::GraphVertexRef,
        name: VERTEX_REF,
        arity: FunctionArity::Exact(3),
        return_type: FunctionReturnType::Unknown,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::GraphEdgeRef,
        name: EDGE_REF,
        arity: FunctionArity::Exact(3),
        return_type: FunctionReturnType::Unknown,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::GraphVertexRef,
        name: VERTEX_REF_TYPED,
        arity: FunctionArity::Exact(3),
        return_type: FunctionReturnType::Unknown,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::GraphEdgeRef,
        name: EDGE_REF_TYPED,
        arity: FunctionArity::Exact(3),
        return_type: FunctionReturnType::Unknown,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::GraphPathToJsonb,
        name: "cassie_graph.path_to_jsonb",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Jsonb,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
];

/// The table functions of the API and their output column names.
pub const TABLE_FUNCTIONS: [&str; 3] = [
    "cassie_graph.key_parts",
    "cassie_graph.path_steps",
    "cassie_graph.weighted_path_steps",
];

/// Whether `name` is one of the FROM-only graph type functions.
pub fn is_table_function(name: &str) -> bool {
    TABLE_FUNCTIONS
        .iter()
        .any(|function| function.eq_ignore_ascii_case(name))
}

/// The argument count of a table function.
pub fn table_arity(name: &str) -> usize {
    if name.eq_ignore_ascii_case(TABLE_FUNCTIONS[2]) {
        2
    } else {
        1
    }
}

/// Whether the `vertex` and `edge` columns of `name` are `graph_element_ref`s.
pub fn has_element_columns(name: &str) -> bool {
    TABLE_FUNCTIONS[1..]
        .iter()
        .any(|function| function.eq_ignore_ascii_case(name))
}

/// Output columns of a table function with the element columns untyped, for callers with
/// no catalog. The descriptor is per database, so the binder resolves it and the typed
/// columns come from [`table_columns_typed`].
pub fn table_columns(name: &str) -> Option<Vec<(String, DataType)>> {
    table_columns_typed(name, &DataType::Null)
}

/// Output columns of a table function whose `vertex` and `edge` columns have the
/// `element` type.
pub fn table_columns_typed(name: &str, element: &DataType) -> Option<Vec<(String, DataType)>> {
    let element = element.clone();
    let columns: Vec<(&str, DataType)> = if name.eq_ignore_ascii_case(TABLE_FUNCTIONS[0]) {
        vec![
            ("ordinal", DataType::BigInt),
            ("type_tag", DataType::SmallInt),
            ("integer_value", DataType::BigInt),
            ("text_value", DataType::Text),
            ("uuid_value", DataType::Uuid),
        ]
    } else if name.eq_ignore_ascii_case(TABLE_FUNCTIONS[1]) {
        vec![
            ("position", DataType::BigInt),
            ("direction", DataType::SmallInt),
            ("vertex", element.clone()),
            ("edge", element),
        ]
    } else if name.eq_ignore_ascii_case(TABLE_FUNCTIONS[2]) {
        vec![
            ("position", DataType::BigInt),
            ("direction", DataType::SmallInt),
            ("vertex", element.clone()),
            ("edge", element),
            (
                "cumulative_cost",
                DataType::Extended(crate::types::extended::ExtendedType::Numeric(None)),
            ),
        ]
    } else {
        return None;
    };
    Some(
        columns
            .into_iter()
            .map(|(name, data_type)| (name.to_owned(), data_type))
            .collect(),
    )
}
