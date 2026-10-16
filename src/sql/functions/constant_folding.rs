//! The existing immutable scalar builtins qualified by constant planning.
use super::FunctionId;

pub(crate) fn is_immutable_scalar(id: FunctionId) -> bool {
    matches!(
        id,
        FunctionId::Coalesce
            | FunctionId::Length
            | FunctionId::Lower
            | FunctionId::Upper
            | FunctionId::Substring
            | FunctionId::Trim
            | FunctionId::Ltrim
            | FunctionId::Rtrim
            | FunctionId::SplitPart
            | FunctionId::Strpos
            | FunctionId::Left
            | FunctionId::Right
            | FunctionId::OctetLength
            | FunctionId::BitLength
            | FunctionId::Ascii
            | FunctionId::Chr
            | FunctionId::StartsWith
            | FunctionId::NullIf
            | FunctionId::Greatest
            | FunctionId::Least
            | FunctionId::Abs
    )
}
