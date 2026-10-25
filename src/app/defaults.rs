use super::{Cassie, CassieError, FieldConstraint};

pub(super) fn apply_default_values(
    app: &Cassie,
    payload: &mut serde_json::Value,
    constraints: &[FieldConstraint],
) -> Result<(), CassieError> {
    let object = payload.as_object_mut().ok_or_else(|| {
        CassieError::InvalidVector("document payload must be a JSON object".to_string())
    })?;

    crate::sql::definition_guard::constraints(constraints)?;
    for constraint in constraints {
        if object.contains_key(&constraint.field) {
            continue;
        }

        if let Some(sequence) = constraint.default_sequence.as_deref() {
            let (value, updated) = app.midge.update_sequence_retrying(sequence, |metadata| {
                metadata.advance().map_err(Into::into)
            })?;
            app.catalog.register_sequence(updated);
            object.insert(
                constraint.field.clone(),
                serde_json::Value::Number(value.into()),
            );
        } else if let Some(default) = &constraint.default_value {
            object.insert(constraint.field.clone(), default.clone());
        } else if let Some(value) = constraint
            .default_expression
            .as_deref()
            .and_then(crate::catalog::evaluate_volatile_default)
        {
            object.insert(constraint.field.clone(), value);
        }
    }

    Ok(())
}

impl Cassie {
    /// The value `column = DEFAULT` assigns to `field`, or `None` when the
    /// column has no default (SQL NULL).
    ///
    /// # Errors
    ///
    /// Returns an error when a sequence default cannot advance.
    pub(crate) fn column_default_json(
        &self,
        collection: &str,
        field: &str,
    ) -> Result<Option<serde_json::Value>, CassieError> {
        let constraints = self
            .catalog
            .get_constraints(collection)
            .into_iter()
            .filter(|constraint| constraint.field == field)
            .collect::<Vec<_>>();
        let mut payload = serde_json::Value::Object(serde_json::Map::new());
        apply_default_values(self, &mut payload, &constraints)?;
        Ok(payload.get(field).cloned())
    }
}

impl Cassie {
    /// `payload` with every missing column default applied, as the proposed
    /// row `EXCLUDED` names in `INSERT ... ON CONFLICT DO UPDATE`.
    ///
    /// # Errors
    ///
    /// Returns an error when a sequence default cannot advance.
    pub(crate) fn payload_with_column_defaults(
        &self,
        collection: &str,
        payload: &serde_json::Value,
    ) -> Result<serde_json::Value, CassieError> {
        self.apply_insert_defaults(collection, payload.clone(), &[])
    }

    /// Applies defaults while treating listed JSON fields as explicit SQL NULL.
    ///
    /// # Errors
    ///
    /// Returns an error when a sequence default cannot advance.
    pub(crate) fn apply_insert_defaults(
        &self,
        collection: &str,
        mut payload: serde_json::Value,
        explicit_json_nulls: &[&str],
    ) -> Result<serde_json::Value, CassieError> {
        let object = payload.as_object_mut().ok_or_else(|| {
            CassieError::InvalidVector("document payload must be a JSON object".to_string())
        })?;
        for field in explicit_json_nulls {
            object.insert((*field).to_string(), serde_json::Value::Null);
        }
        let constraints = self.catalog.get_constraints(collection);
        if !constraints.is_empty() {
            apply_default_values(self, &mut payload, &constraints)?;
        }
        if let Some(object) = payload.as_object_mut() {
            for field in explicit_json_nulls {
                object.remove(*field);
            }
        }
        Ok(payload)
    }
}
