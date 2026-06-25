use super::*;

pub(super) fn apply_default_values(
    app: &Cassie,
    payload: &mut serde_json::Value,
    constraints: &[FieldConstraint],
) -> Result<(), CassieError> {
    let object = payload.as_object_mut().ok_or_else(|| {
        CassieError::InvalidVector("document payload must be a JSON object".to_string())
    })?;

    for constraint in constraints {
        if object.contains_key(&constraint.field) {
            continue;
        }

        if let Some(sequence) = constraint.default_sequence.as_deref() {
            let value = app.midge.next_sequence_value(sequence)?;
            app.catalog.set_sequence_current_value(sequence, value);
            object.insert(
                constraint.field.clone(),
                serde_json::Value::Number(value.into()),
            );
        } else if let Some(default) = &constraint.default_value {
            object.insert(constraint.field.clone(), default.clone());
        }
    }

    Ok(())
}
