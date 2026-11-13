//! The operand family each declared data type belongs to.
use super::{DataType, OperandFamily};

pub(in crate::sql::binder) fn data_type_family(data_type: &DataType) -> OperandFamily {
    match data_type {
        DataType::SmallInt
        | DataType::Int
        | DataType::BigInt
        | DataType::Float
        | DataType::ObjectId(_) => OperandFamily::Numeric,
        DataType::Extended(kind) => match kind.family() {
            crate::types::extended::ExtendedFamily::Numeric => OperandFamily::Numeric,
            crate::types::extended::ExtendedFamily::Temporal
            | crate::types::extended::ExtendedFamily::Timespan => OperandFamily::Temporal,
            family => OperandFamily::Typed(family),
        },
        DataType::Boolean => OperandFamily::Boolean,
        DataType::Name | DataType::Text | DataType::Char { .. } | DataType::Varchar { .. } => {
            OperandFamily::Text
        }
        DataType::Date | DataType::Time | DataType::Timestamp => OperandFamily::Temporal,
        DataType::Vector(_) => OperandFamily::Vector,
        DataType::Json | DataType::Jsonb => OperandFamily::Json,
        DataType::JsonPath
        | DataType::Uuid
        | DataType::Bytea
        | DataType::Array(_)
        | DataType::Enum(_)
        | DataType::Null => OperandFamily::Other,
        DataType::Composite(_) => OperandFamily::Record,
        DataType::Domain(descriptor) => data_type_family(&descriptor.base),
    }
}
