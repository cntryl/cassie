//! Qualified input rules are separate from the public return metadata.

use super::FunctionId;
use crate::types::extended::ExtendedType;
use crate::types::DataType;

pub(crate) fn aggregate_accepts_input(id: FunctionId, data_type: &DataType) -> bool {
    if super::temporal::aggregate_input(id, data_type) {
        return true;
    }
    match id {
        id if super::statistical::is_statistical_function(id)
            || super::regression::is_regression_function(id) =>
        {
            matches!(
                data_type,
                DataType::Null
                    | DataType::SmallInt
                    | DataType::Int
                    | DataType::BigInt
                    | DataType::Float
                    | DataType::Extended(ExtendedType::Numeric(_) | ExtendedType::Real)
            )
        }
        FunctionId::Sum | FunctionId::Avg => matches!(
            data_type,
            DataType::Null
                | DataType::SmallInt
                | DataType::Int
                | DataType::BigInt
                | DataType::Float
                | DataType::Extended(ExtendedType::Numeric(_) | ExtendedType::Real)
        ),
        FunctionId::BitAnd | FunctionId::BitOr | FunctionId::BitXor => matches!(
            data_type,
            DataType::Null | DataType::SmallInt | DataType::Int | DataType::BigInt
        ),
        FunctionId::Min
        | FunctionId::Max
        | FunctionId::Mode
        | FunctionId::Rank
        | FunctionId::DenseRank
        | FunctionId::PercentRank
        | FunctionId::CumeDist => matches!(
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
            DataType::Null
                | DataType::Text
                | DataType::Char { .. }
                | DataType::Varchar { .. }
                | DataType::Bytea
        ),
        _ => true,
    }
}

pub(crate) fn has_distinct_equality(data_type: &DataType) -> bool {
    match data_type {
        DataType::Json | DataType::Jsonb => false,
        DataType::Array(element) => has_distinct_equality(element),
        _ => true,
    }
}

#[derive(Clone, Copy)]
pub(crate) enum FunctionInputSignature {
    Text,
    Boolean,
    AnyElement,
    CommonArguments,
    PercentileFraction,
    OrderedSetOrderKey,
    HypotheticalSetArguments,
    BitwiseInteger,
    RegexpReplace,
    Positional(&'static [i32]),
    VariadicTail { first: i32, tail: i32 },
}

impl FunctionInputSignature {
    pub(crate) fn argument_oid(self, index: usize) -> Option<i32> {
        match self {
            Self::Positional(oids) => oids.get(index).copied(),
            Self::VariadicTail { first, tail } => Some(if index == 0 { first } else { tail }),
            _ => None,
        }
    }
}

pub(crate) const fn input_signature(id: FunctionId) -> Option<FunctionInputSignature> {
    match id {
        FunctionId::Json(super::json::JsonFunction::ExtractPath { binary, .. }) => {
            Some(FunctionInputSignature::VariadicTail {
                first: if binary { 3802 } else { 114 },
                tail: 25,
            })
        }
        FunctionId::Json(super::json::JsonFunction::ToJson(_)) => {
            Some(FunctionInputSignature::AnyElement)
        }
        FunctionId::Json(kind) => match super::json::inputs(kind) {
            Some(oids) => Some(FunctionInputSignature::Positional(oids)),
            None => None,
        },
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
        FunctionId::PgSleep => Some(FunctionInputSignature::Positional(&[701])),
        FunctionId::PgSleepFor => Some(FunctionInputSignature::Positional(&[1186])),
        FunctionId::PgSleepUntil => Some(FunctionInputSignature::Positional(&[1184])),
        FunctionId::Power => Some(FunctionInputSignature::Positional(&[701, 701])),
        FunctionId::PgInputIsValid => Some(FunctionInputSignature::Positional(&[25, 25])),
        FunctionId::PgCharToEncoding
        | FunctionId::PgSizeBytes
        | FunctionId::Reverse
        | FunctionId::Initcap
        | FunctionId::Length
        | FunctionId::OctetLength
        | FunctionId::BitLength
        | FunctionId::Ascii
        | FunctionId::Format => Some(FunctionInputSignature::Positional(&[25])),
        FunctionId::PgSizePretty => Some(FunctionInputSignature::Positional(&[20])),
        FunctionId::PgGetFunctionArguments
        | FunctionId::PgGetFunctionIdentityArguments
        | FunctionId::PgGetFunctionResult
        | FunctionId::PgIsOtherTempSchema => Some(FunctionInputSignature::Positional(&[26])),
        FunctionId::PgMyTempSchema => Some(FunctionInputSignature::Positional(&[])),
        FunctionId::Lower | FunctionId::ConcatWs => Some(FunctionInputSignature::Text),
        FunctionId::BoolAnd | FunctionId::BoolOr => Some(FunctionInputSignature::Boolean),
        FunctionId::AnyValue => Some(FunctionInputSignature::AnyElement),
        FunctionId::BitAnd | FunctionId::BitOr | FunctionId::BitXor => {
            Some(FunctionInputSignature::BitwiseInteger)
        }
        id if super::regression::is_regression_function(id) => {
            Some(FunctionInputSignature::Positional(&[701, 701]))
        }
        id if super::statistical::is_statistical_function(id) => {
            Some(FunctionInputSignature::CommonArguments)
        }
        FunctionId::PercentileCont | FunctionId::PercentileDisc => {
            Some(FunctionInputSignature::PercentileFraction)
        }
        FunctionId::Mode => Some(FunctionInputSignature::OrderedSetOrderKey),
        FunctionId::Rank
        | FunctionId::DenseRank
        | FunctionId::PercentRank
        | FunctionId::CumeDist => Some(FunctionInputSignature::HypotheticalSetArguments),
        FunctionId::Coalesce => Some(FunctionInputSignature::CommonArguments),
        FunctionId::Substring => Some(FunctionInputSignature::Positional(&[25, 23, 23])),
        FunctionId::Decode
        | FunctionId::StartsWith
        | FunctionId::Trim
        | FunctionId::Ltrim
        | FunctionId::Rtrim
        | FunctionId::Strpos => Some(FunctionInputSignature::Positional(&[25, 25])),
        FunctionId::SplitPart => Some(FunctionInputSignature::Positional(&[25, 25, 23])),
        FunctionId::Replace | FunctionId::Translate => {
            Some(FunctionInputSignature::Positional(&[25, 25, 25]))
        }
        FunctionId::Left | FunctionId::Right | FunctionId::Repeat => {
            Some(FunctionInputSignature::Positional(&[25, 23]))
        }
        FunctionId::Lpad | FunctionId::Rpad => {
            Some(FunctionInputSignature::Positional(&[25, 23, 25]))
        }
        FunctionId::Overlay => Some(FunctionInputSignature::Positional(&[25, 25, 23, 23])),
        FunctionId::Chr => Some(FunctionInputSignature::Positional(&[23])),
        FunctionId::RegexpLike => Some(FunctionInputSignature::Positional(&[25, 25, 25])),
        FunctionId::RegexpCount => Some(FunctionInputSignature::Positional(&[25, 25, 23, 25])),
        FunctionId::RegexpReplace => Some(FunctionInputSignature::RegexpReplace),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{input_signature, FunctionInputSignature};
    use crate::sql::functions::FunctionId;

    fn positional(id: FunctionId) -> &'static [i32] {
        match input_signature(id) {
            Some(FunctionInputSignature::Positional(oids)) => oids,
            _ => panic!("expected positional input signature for {id:?}"),
        }
    }

    #[test]
    fn should_define_postgresql_input_types_for_text_transform_functions() {
        // Arrange
        let signatures = [
            (FunctionId::Overlay, &[25, 25, 23, 23][..]),
            (FunctionId::Lpad, &[25, 23, 25][..]),
            (FunctionId::Rpad, &[25, 23, 25][..]),
            (FunctionId::Repeat, &[25, 23][..]),
            (FunctionId::Reverse, &[25][..]),
            (FunctionId::Initcap, &[25][..]),
            (FunctionId::Translate, &[25, 25, 25][..]),
        ];

        // Act
        // Assert
        for (function, expected) in signatures {
            assert_eq!(positional(function), expected, "{function:?}");
        }
        assert!(matches!(
            input_signature(FunctionId::ConcatWs),
            Some(FunctionInputSignature::Text)
        ));
    }

    #[test]
    fn should_define_postgresql_input_types_for_regex_functions() {
        // Arrange
        let signatures = [
            (FunctionId::RegexpLike, &[25, 25, 25][..]),
            (FunctionId::RegexpCount, &[25, 25, 23, 25][..]),
        ];

        // Act
        // Assert
        for (function, expected) in signatures {
            assert_eq!(positional(function), expected, "{function:?}");
        }
        assert!(matches!(
            input_signature(FunctionId::RegexpReplace),
            Some(FunctionInputSignature::RegexpReplace)
        ));
    }

    #[test]
    fn should_define_postgresql_input_types_for_sleep_functions() {
        // Arrange
        let signatures = [
            (FunctionId::PgSleep, &[701][..]),
            (FunctionId::PgSleepFor, &[1186][..]),
            (FunctionId::PgSleepUntil, &[1184][..]),
        ];

        // Act
        // Assert
        for (function, expected) in signatures {
            assert_eq!(positional(function), expected, "{function:?}");
        }
    }

    #[test]
    fn should_define_within_group_input_signatures_for_ordered_aggregates() {
        // Arrange
        let hypothetical = [
            FunctionId::Rank,
            FunctionId::DenseRank,
            FunctionId::PercentRank,
            FunctionId::CumeDist,
        ];

        // Act
        // Assert
        assert!(matches!(
            input_signature(FunctionId::Mode),
            Some(FunctionInputSignature::OrderedSetOrderKey)
        ));
        for id in hypothetical {
            assert!(matches!(
                input_signature(id),
                Some(FunctionInputSignature::HypotheticalSetArguments)
            ));
        }
    }

    #[test]
    fn should_define_postgresql_polymorphic_input_for_any_value() {
        // Arrange
        let function_id = FunctionId::AnyValue;

        // Act
        let signature = input_signature(function_id);

        // Assert
        assert!(matches!(
            signature,
            Some(FunctionInputSignature::AnyElement)
        ));
    }
}
