//! Classification of existing builtin and private catalog types.
use crate::catalog::builtin_identity::BuiltinTypeRow;
use crate::types::DataType;

pub(super) fn kind(data_type: &DataType, builtin: Option<&BuiltinTypeRow>) -> String {
    if let Some(metadata) = builtin {
        return metadata.typtype.to_string();
    }
    if matches!(data_type, DataType::Array(_)) {
        "a".to_string()
    } else {
        "b".to_string()
    }
}

pub(super) fn category(data_type: &DataType, builtin: Option<&BuiltinTypeRow>) -> String {
    if let Some(metadata) = builtin {
        return metadata.category.to_string();
    }
    match data_type {
        DataType::Null => "Z".to_string(),
        DataType::Boolean => "B".to_string(),
        DataType::SmallInt
        | DataType::Int
        | DataType::BigInt
        | DataType::Float
        | DataType::ObjectId(_) => "N".to_string(),
        DataType::Name
        | DataType::Text
        | DataType::Char { .. }
        | DataType::Varchar { .. }
        | DataType::Json => "S".to_string(),
        // pgvector's CREATE TYPE definition does not override CATEGORY, so
        // PostgreSQL assigns vector the user-defined base-type category.
        DataType::Bytea
        | DataType::Uuid
        | DataType::Jsonb
        | DataType::JsonPath
        | DataType::Vector(_) => "U".to_string(),
        DataType::Date | DataType::Time | DataType::Timestamp => "D".to_string(),
        DataType::Array(_) => "A".to_string(),
        DataType::Extended(kind) => kind.category().to_string(),
    }
}
