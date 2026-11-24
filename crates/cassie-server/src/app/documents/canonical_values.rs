use crate::catalog::FieldMeta;
use crate::types::{extended::ExtendedType, DataType};

/// Rewrites typed text values in `payload` to the canonical form storage
/// persists, before validation, UNIQUE checks, staging and RETURNING see it.
///
/// - `CHAR(n)` drops insignificant trailing blanks, which also lets a value
///   such as `'x  '` fit `CHAR(1)`, as PostgreSQL allows.
/// - `DATE`, `TIME` and `TIMESTAMP` use the shared temporal canonical forms.
/// - `UUID` becomes lowercase hyphenated text and `BYTEA` lowercase hex.
/// - `NUMERIC` JSON values use their typmod-aware decimal text storage form.
///
/// Values that do not parse are left unchanged so schema validation reports
/// them with its usual error.
pub(super) fn canonicalize_typed_values(fields: &[FieldMeta], payload: &mut serde_json::Value) {
    let Some(object) = payload.as_object_mut() else {
        return;
    };
    for field in fields {
        if let DataType::Extended(kind @ ExtendedType::Numeric(_)) = field.data_type {
            let canonical_numeric = object
                .get(&field.name)
                .and_then(|value| kind.from_json(value).ok())
                .map(|typed| typed.to_json());
            if let Some(canonical_numeric) = canonical_numeric {
                object.insert(field.name.clone(), canonical_numeric);
            }
            continue;
        }
        let Some(serde_json::Value::String(text)) = object.get_mut(&field.name) else {
            continue;
        };
        match field.data_type {
            DataType::Char { .. } => {
                let canonical_len = crate::types::char_text::canonical_char_text(text).len();
                text.truncate(canonical_len);
            }
            DataType::Date => replace_if_ok(text, crate::types::temporal::canonical_date(text)),
            DataType::Time => replace_if_ok(text, crate::types::temporal::canonical_time(text)),
            DataType::Timestamp => {
                replace_if_ok(text, crate::types::temporal::canonical_timestamp(text));
            }
            DataType::Uuid => replace_if_ok(
                text,
                uuid::Uuid::parse_str(text).map(|uuid| uuid.to_string()),
            ),
            DataType::Bytea => {
                replace_if_ok(text, crate::midge::row_blob::canonical_bytea_text(text));
            }
            _ => {}
        }
    }
}

fn replace_if_ok<E>(text: &mut String, canonical: Result<String, E>) {
    if let Ok(canonical) = canonical {
        *text = canonical;
    }
}
