//! Qualified input rules are separate from the public return metadata.

use super::FunctionId;
use crate::types::DataType;

pub(crate) fn aggregate_accepts_input(id: FunctionId, data_type: &DataType) -> bool {
    match id {
        FunctionId::Sum | FunctionId::Avg => matches!(
            data_type,
            DataType::Null
                | DataType::SmallInt
                | DataType::Int
                | DataType::BigInt
                | DataType::Float
        ),
        FunctionId::Min | FunctionId::Max => matches!(
            data_type,
            DataType::Null
                | DataType::SmallInt
                | DataType::Int
                | DataType::BigInt
                | DataType::Float
                | DataType::Text
                | DataType::Char { .. }
                | DataType::Varchar { .. }
                | DataType::Bytea
                | DataType::Date
                | DataType::Time
                | DataType::Timestamp
                | DataType::Array(_)
                // Cassie's established vector extension retains row extrema.
                | DataType::Vector(_)
        ),
        FunctionId::BoolAnd | FunctionId::BoolOr => {
            matches!(data_type, DataType::Null | DataType::Boolean)
        }
        _ => true,
    }
}

pub(crate) fn has_distinct_equality(data_type: &DataType) -> bool {
    match data_type {
        DataType::Json => false,
        DataType::Array(element) => has_distinct_equality(element),
        _ => true,
    }
}

#[derive(Clone, Copy)]
pub(crate) enum FunctionInputSignature {
    Text,
    Boolean,
    CommonArguments,
    Positional(&'static [i32]),
}

pub(crate) const fn input_signature(id: FunctionId) -> Option<FunctionInputSignature> {
    match id {
        FunctionId::PgCharToEncoding | FunctionId::PgSizeBytes => {
            Some(FunctionInputSignature::Positional(&[25]))
        }
        FunctionId::PgSizePretty => Some(FunctionInputSignature::Positional(&[20])),
        FunctionId::Lower => Some(FunctionInputSignature::Text),
        FunctionId::BoolAnd | FunctionId::BoolOr => Some(FunctionInputSignature::Boolean),
        FunctionId::Coalesce => Some(FunctionInputSignature::CommonArguments),
        FunctionId::Substring => Some(FunctionInputSignature::Positional(&[25, 23, 23])),
        FunctionId::Trim | FunctionId::Ltrim | FunctionId::Rtrim | FunctionId::Strpos => {
            Some(FunctionInputSignature::Positional(&[25, 25]))
        }
        FunctionId::SplitPart => Some(FunctionInputSignature::Positional(&[25, 25, 23])),
        FunctionId::Left | FunctionId::Right => Some(FunctionInputSignature::Positional(&[25, 23])),
        FunctionId::Length
        | FunctionId::OctetLength
        | FunctionId::BitLength
        | FunctionId::Ascii => Some(FunctionInputSignature::Positional(&[25])),
        FunctionId::Chr => Some(FunctionInputSignature::Positional(&[23])),
        FunctionId::StartsWith => Some(FunctionInputSignature::Positional(&[25, 25])),
        _ => None,
    }
}
