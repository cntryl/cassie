use super::{Cassie, CassieError, FieldConstraint};

const CONSTRAINT_SCAN_BATCH_SIZE: usize = 1024;

impl Cassie {
    /// Validates existing rows against newly added CHECK and NOT NULL constraints.
    pub(crate) fn validate_existing_check_and_not_null_rows(
        &self,
        collection: &str,
        constraints: &[FieldConstraint],
    ) -> Result<(), CassieError> {
        if constraints.iter().all(|constraint| {
            !constraint.primary_key && !constraint.not_null && constraint.check.is_none()
        }) {
            return Ok(());
        }

        for document in self
            .scan_documents_batched_for_session(None, collection, CONSTRAINT_SCAN_BATCH_SIZE)?
            .into_iter()
            .flatten()
        {
            let object = document.payload.as_object().ok_or_else(|| {
                CassieError::InvalidVector("document payload must be a JSON object".to_string())
            })?;
            for constraint in constraints {
                let existing = object.get(&constraint.field);
                if (constraint.not_null || constraint.primary_key)
                    && existing.is_none_or(serde_json::Value::is_null)
                {
                    let kind = if constraint.primary_key {
                        "PRIMARY KEY"
                    } else {
                        "NOT NULL"
                    };
                    return Err(CassieError::NotNullViolation {
                        table: collection.to_string(),
                        column: constraint.field.clone(),
                        constraint: Some(crate::catalog::generated_constraint_name(
                            collection,
                            &constraint.field,
                            kind,
                        )),
                    });
                }

                if let (Some(check), Some(value)) = (&constraint.check, existing) {
                    if !Self::satisfies_check_constraint(value, check)? {
                        return Err(CassieError::CheckViolation {
                            table: collection.to_string(),
                            column: check.field.clone(),
                            constraint: crate::catalog::generated_constraint_name(
                                collection,
                                &check.field,
                                "CHECK",
                            ),
                        });
                    }
                }
            }
        }
        Ok(())
    }
}
