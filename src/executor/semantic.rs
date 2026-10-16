pub(crate) use crate::types::semantic::*;

/// SQL bpchar compares without trailing blanks while retaining its original
/// padded value for result encoding. Other scalar identities use existing keys.
pub(crate) fn typed_value(
    value: &crate::types::Value,
    data_type: Option<&crate::types::DataType>,
) -> SemanticValue {
    if let (crate::types::Value::String(text), Some(crate::types::DataType::Char { .. })) =
        (value, data_type)
    {
        return SemanticValue::String(
            crate::types::char_text::canonical_char_text(text).to_owned(),
        );
    }
    SemanticValue::from_value(value)
}
