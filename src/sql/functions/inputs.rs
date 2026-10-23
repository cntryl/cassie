//! Qualified input rules are separate from the public return metadata.

use super::FunctionId;
use crate::types::extended::ExtendedType;
use crate::types::DataType;

pub(crate) fn aggregate_accepts_input(id: FunctionId, data_type: &DataType) -> bool {
    if super::temporal::aggregate_input(id, data_type) {
        return true;
    }
    match id {
        FunctionId::Sum | FunctionId::Avg => matches!(
            data_type,
            DataType::Null
                | DataType::SmallInt
                | DataType::Int
                | DataType::BigInt
                | DataType::Float
                | DataType::Extended(ExtendedType::Numeric(_) | ExtendedType::Real)
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
                | DataType::Extended(ExtendedType::Numeric(_) | ExtendedType::Real)
        ),
        FunctionId::BoolAnd | FunctionId::BoolOr => {
            matches!(data_type, DataType::Null | DataType::Boolean)
        }
        FunctionId::StringAgg => matches!(
            data_type,
            DataType::Null | DataType::Text | DataType::Char { .. } | DataType::Varchar { .. }
        ),
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
        FunctionId::Encode => Some(FunctionInputSignature::Positional(&[17, 25])),
        FunctionId::GetByte => Some(FunctionInputSignature::Positional(&[17, 23])),
        FunctionId::SetByte => Some(FunctionInputSignature::Positional(&[17, 23, 23])),
        FunctionId::GetBit => Some(FunctionInputSignature::Positional(&[17, 20])),
        FunctionId::SetBit => Some(FunctionInputSignature::Positional(&[17, 20, 23])),
        FunctionId::BitCount
        | FunctionId::Sha224
        | FunctionId::Sha256
        | FunctionId::Sha384
        | FunctionId::Sha512 => Some(FunctionInputSignature::Positional(&[17])),
        FunctionId::Power => Some(FunctionInputSignature::Positional(&[701, 701])),
        FunctionId::PgCharToEncoding | FunctionId::PgSizeBytes => {
            Some(FunctionInputSignature::Positional(&[25]))
        }
        FunctionId::PgSizePretty => Some(FunctionInputSignature::Positional(&[20])),
        FunctionId::Lower => Some(FunctionInputSignature::Text),
        FunctionId::BoolAnd | FunctionId::BoolOr => Some(FunctionInputSignature::Boolean),
        FunctionId::Coalesce => Some(FunctionInputSignature::CommonArguments),
        FunctionId::Substring => Some(FunctionInputSignature::Positional(&[25, 23, 23])),
        FunctionId::StringAgg
        | FunctionId::Decode
        | FunctionId::StartsWith
        | FunctionId::Trim
        | FunctionId::Ltrim
        | FunctionId::Rtrim
        | FunctionId::Strpos => Some(FunctionInputSignature::Positional(&[25, 25])),
        FunctionId::SplitPart => Some(FunctionInputSignature::Positional(&[25, 25, 23])),
        FunctionId::Replace => Some(FunctionInputSignature::Positional(&[25, 25, 25])),
        FunctionId::Left | FunctionId::Right => Some(FunctionInputSignature::Positional(&[25, 23])),
        FunctionId::Length
        | FunctionId::OctetLength
        | FunctionId::BitLength
        | FunctionId::Ascii
        | FunctionId::Format => Some(FunctionInputSignature::Positional(&[25])),
        FunctionId::Chr => Some(FunctionInputSignature::Positional(&[23])),
        _ => None,
    }
}
