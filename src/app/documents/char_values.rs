use crate::catalog::FieldMeta;
use crate::types::DataType;

/// Rewrites every `CHAR(n)` value in `payload` to its canonical form without
/// trailing blanks, before validation, UNIQUE checks and storage see it.
///
/// Trailing blanks are insignificant in `CHAR(n)`, so this also lets a value
/// such as `'x  '` fit `CHAR(1)`, as PostgreSQL allows.
pub(super) fn canonicalize_char_values(fields: &[FieldMeta], payload: &mut serde_json::Value) {
    let Some(object) = payload.as_object_mut() else {
        return;
    };
    for field in fields {
        if !matches!(field.data_type, DataType::Char { .. }) {
            continue;
        }
        if let Some(serde_json::Value::String(text)) = object.get_mut(&field.name) {
            let canonical_len = crate::types::char_text::canonical_char_text(text).len();
            text.truncate(canonical_len);
        }
    }
}
