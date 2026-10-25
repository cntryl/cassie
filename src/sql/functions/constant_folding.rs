//! The existing immutable scalar builtins qualified by constant planning.
use super::FunctionId;

pub(crate) fn is_immutable_scalar(id: FunctionId) -> bool {
    super::is_bytea_function(id)
        || matches!(
            id,
            FunctionId::UnicodeVersion
                | FunctionId::PgSizeBytes
                | FunctionId::PgSizePretty
                | FunctionId::PgGetFunctionArguments
                | FunctionId::PgGetFunctionIdentityArguments
                | FunctionId::PgGetFunctionResult
                | FunctionId::Coalesce
                | FunctionId::Length
                | FunctionId::Lower
                | FunctionId::Upper
                | FunctionId::Substring
                | FunctionId::Trim
                | FunctionId::Ltrim
                | FunctionId::Rtrim
                | FunctionId::SplitPart
                | FunctionId::Strpos
                | FunctionId::Replace
                | FunctionId::Format
                | FunctionId::Left
                | FunctionId::Right
                | FunctionId::Overlay
                | FunctionId::Lpad
                | FunctionId::Rpad
                | FunctionId::Repeat
                | FunctionId::Reverse
                | FunctionId::Initcap
                | FunctionId::Translate
                | FunctionId::ConcatWs
                | FunctionId::OctetLength
                | FunctionId::BitLength
                | FunctionId::Ascii
                | FunctionId::Chr
                | FunctionId::StartsWith
                | FunctionId::NullIf
                | FunctionId::Greatest
                | FunctionId::Least
                | FunctionId::Abs
                | FunctionId::Power
        )
}
