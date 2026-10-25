//! Fixed builtin result identities shared by binding and output admission.
use super::FunctionReturnType;
use crate::types::DataType;

impl FunctionReturnType {
    pub(crate) const fn fixed_type(self) -> Option<DataType> {
        Some(match self {
            Self::Json => DataType::Json,
            Self::Jsonb => DataType::Jsonb,
            Self::Bytea => DataType::Bytea,
            Self::Float => DataType::Float,
            Self::Text => DataType::Text,
            Self::Int => DataType::Int,
            Self::BigInt => DataType::BigInt,
            Self::Boolean => DataType::Boolean,
            Self::Timestamp => DataType::Timestamp,
            Self::ObjectId(kind) => DataType::ObjectId(kind),
            Self::FirstNonNullArgument
            | Self::FirstComparedArgument
            | Self::NumericArgument
            | Self::SumArgument
            | Self::Math
            | Self::Unknown => return None,
        })
    }
}
