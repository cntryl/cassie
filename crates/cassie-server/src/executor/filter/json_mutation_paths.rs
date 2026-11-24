//! Finite TEXT[] traversal shared by JSONB mutation functions.
use crate::types::extended::TypeInputError;
use crate::{app::CassieError, executor::QueryError, runtime::QueryExecutionControls};
use serde_json::Value;

#[derive(Clone, Copy)]
pub(super) enum Mode {
    Set(bool),
    Insert(bool),
    Delete,
}

pub(super) fn apply(
    document: &mut Value,
    path: &[Value],
    replacement: Option<Value>,
    mode: Mode,
    controls: Option<&QueryExecutionControls>,
) -> Result<(), QueryError> {
    if !matches!(document, Value::Array(_) | Value::Object(_)) {
        return Err(CassieError::InvalidParameterValue("cannot set path in scalar".into()).into());
    }
    let empty = match document {
        Value::Array(values) => values.is_empty(),
        Value::Object(fields) => fields.is_empty(),
        _ => false,
    };
    if empty && matches!(mode, Mode::Set(false) | Mode::Delete) {
        return Ok(());
    }
    walk(document, path, replacement, mode, 0, controls)
}

fn walk(
    document: &mut Value,
    path: &[Value],
    replacement: Option<Value>,
    mode: Mode,
    level: usize,
    controls: Option<&QueryExecutionControls>,
) -> Result<(), QueryError> {
    if let Some(controls) = controls {
        controls.check_interruption()?;
    }
    let Some((part, remaining)) = path.split_first() else {
        return Ok(());
    };
    let key = part.as_str().ok_or_else(|| {
        CassieError::TypeInput(TypeInputError::new(
            "22004",
            format!("path element at position {} is null", level + 1),
        ))
    })?;
    match document {
        Value::Object(fields) => {
            if remaining.is_empty() {
                match mode {
                    Mode::Set(create) if create || fields.contains_key(key) => {
                        fields.insert(key.to_owned(), replacement.expect("set replacement"));
                    }
                    Mode::Insert(_) => {
                        if fields.contains_key(key) {
                            return Err(CassieError::InvalidParameterValue(
                                "cannot replace existing key".into(),
                            )
                            .into());
                        }
                        fields.insert(key.to_owned(), replacement.expect("insert replacement"));
                    }
                    Mode::Delete => {
                        fields.remove(key);
                    }
                    Mode::Set(_) => {}
                }
            } else if let Some(child) = fields.get_mut(key) {
                walk(child, remaining, replacement, mode, level + 1, controls)?;
            }
        }
        Value::Array(values) => {
            let index = array_index(key, level)?;
            let length = i64::try_from(values.len()).unwrap_or(i64::MAX);
            let position = if index < 0 {
                length + i64::from(index)
            } else {
                i64::from(index)
            };
            if remaining.is_empty() {
                array_final(values, position, replacement, mode);
            } else if let Ok(index) = usize::try_from(position) {
                if let Some(child) = values.get_mut(index) {
                    walk(child, remaining, replacement, mode, level + 1, controls)?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn array_final(values: &mut Vec<Value>, position: i64, replacement: Option<Value>, mode: Mode) {
    let index = usize::try_from(position).ok();
    match mode {
        Mode::Set(create) => {
            if let Some(slot) = index.and_then(|index| values.get_mut(index)) {
                *slot = replacement.expect("set replacement");
            } else if create {
                let value = replacement.expect("set replacement");
                if position < 0 {
                    values.insert(0, value);
                } else {
                    values.push(value);
                }
            }
        }
        Mode::Insert(after) => {
            let index = index.map_or(0, |index| {
                index.saturating_add(usize::from(after)).min(values.len())
            });
            values.insert(index, replacement.expect("insert replacement"));
        }
        Mode::Delete => {
            if let Some(index) = index.filter(|index| *index < values.len()) {
                values.remove(index);
            }
        }
    }
}

fn array_index(key: &str, level: usize) -> Result<i32, QueryError> {
    key.trim_matches(|ch| matches!(ch, ' ' | '\t' | '\n' | '\r' | '\x0b' | '\x0c'))
        .parse::<i32>()
        .map_err(|_| {
            CassieError::TypeInput(TypeInputError::new(
                "22P02",
                format!(
                    "path element at position {} is not an integer: {key:?}",
                    level + 1
                ),
            ))
            .into()
        })
}
