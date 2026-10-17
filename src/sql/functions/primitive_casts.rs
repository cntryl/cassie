//! Function notation for selected native type names, independent of overload binding.
use crate::sql::ast::{AggregateQuantifier, Expr, FunctionCall};
use crate::types::DataType;

pub(crate) struct PrimitiveCast {
    pub(crate) name: &'static str,
    pub(crate) data_type: DataType,
}

pub(crate) fn candidate(function: &FunctionCall) -> Option<PrimitiveCast> {
    if function.args.len() != 1
        || function.quantifier != AggregateQuantifier::Default
        || function.filter.is_some()
        || !function.order_by.is_empty()
    {
        return None;
    }
    target(&function.name)
}

const TARGETS: &[(&str, DataType)] = &[
    ("int2", DataType::SmallInt),
    ("pg_catalog.int2", DataType::SmallInt),
    ("int4", DataType::Int),
    ("pg_catalog.int4", DataType::Int),
    ("int8", DataType::BigInt),
    ("pg_catalog.int8", DataType::BigInt),
    ("float8", DataType::Float),
    ("pg_catalog.float8", DataType::Float),
    ("bool", DataType::Boolean),
    ("pg_catalog.bool", DataType::Boolean),
    ("text", DataType::Text),
    ("pg_catalog.text", DataType::Text),
    ("date", DataType::Date),
    ("pg_catalog.date", DataType::Date),
    ("uuid", DataType::Uuid),
    ("pg_catalog.uuid", DataType::Uuid),
    ("bytea", DataType::Bytea),
    ("pg_catalog.bytea", DataType::Bytea),
    ("pg_catalog.time", DataType::Time),
    ("pg_catalog.timestamp", DataType::Timestamp),
    ("pg_catalog.varchar", DataType::Varchar { length: None }),
];

fn target(name: &str) -> Option<PrimitiveCast> {
    TARGETS
        .iter()
        .find(|(candidate, _)| name.eq_ignore_ascii_case(candidate))
        .map(|(name, data_type)| PrimitiveCast {
            name,
            data_type: data_type.clone(),
        })
}

pub(crate) fn unknown_literal(expression: &Expr) -> bool {
    matches!(expression, Expr::StringLiteral(_) | Expr::Null)
}
