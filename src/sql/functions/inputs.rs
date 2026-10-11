//! Qualified input rules are separate from the public return metadata.

use super::FunctionId;

#[derive(Clone, Copy)]
pub(crate) enum FunctionInputSignature {
    Text,
    CommonArguments,
}

pub(crate) const fn input_signature(id: FunctionId) -> Option<FunctionInputSignature> {
    match id {
        FunctionId::Lower => Some(FunctionInputSignature::Text),
        FunctionId::Coalesce => Some(FunctionInputSignature::CommonArguments),
        _ => None,
    }
}
