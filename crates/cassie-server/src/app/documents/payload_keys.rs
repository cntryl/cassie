use crate::app::CassieError;
use crate::catalog::FieldMeta;

/// Rewrites top-level payload keys to the declared field names.
///
/// Field lookups downstream (defaults, vector indexing, UNIQUE and FOREIGN KEY
/// checks, row encoding) use exact declared names, so a key that differs only in
/// letter case must be renamed before those steps run. Keys without a matching
/// field are left unchanged so schema validation can reject them. Two keys that
/// resolve to the same field, or one key that matches several fields only by
/// case, are rejected as ambiguous.
pub(super) fn normalize_payload_keys(
    collection: &str,
    fields: &[FieldMeta],
    payload: &mut serde_json::Value,
) -> Result<(), CassieError> {
    let Some(object) = payload.as_object_mut() else {
        return Ok(());
    };
    let is_declared = |key: &str| fields.iter().any(|field| field.name == key);
    if object.keys().all(|key| is_declared(key)) {
        return Ok(());
    }

    let entries = std::mem::take(object);
    for (key, value) in entries {
        let declared = declared_field_name(collection, fields, &key)?;
        let name = declared.map_or(key, str::to_string);
        if object.contains_key(&name) {
            return Err(CassieError::InvalidVector(format!(
                "field '{name}' is specified more than once in the document payload for collection '{collection}'"
            )));
        }
        object.insert(name, value);
    }
    Ok(())
}

fn declared_field_name<'a>(
    collection: &str,
    fields: &'a [FieldMeta],
    key: &str,
) -> Result<Option<&'a str>, CassieError> {
    if let Some(field) = fields.iter().find(|field| field.name == key) {
        return Ok(Some(field.name.as_str()));
    }
    let mut candidates = fields
        .iter()
        .filter(|field| field.name.eq_ignore_ascii_case(key));
    let Some(field) = candidates.next() else {
        return Ok(None);
    };
    if candidates.next().is_some() {
        return Err(CassieError::InvalidVector(format!(
            "field '{key}' matches more than one field on collection '{collection}' by case"
        )));
    }
    Ok(Some(field.name.as_str()))
}
