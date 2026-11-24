use super::{Cassie, CassieError, FieldConstraint};

const CONSTRAINT_SCAN_BATCH_SIZE: usize = 1024;

/// The value every existing row receives when a column is added: the column
/// name and its backfilled default (`Null` when the default is NULL).
#[derive(Clone, Copy)]
pub(crate) struct AddedColumnDefault<'a> {
    pub(crate) field: &'a str,
    pub(crate) value: &'a serde_json::Value,
}

impl<'a> AddedColumnDefault<'a> {
    pub(crate) fn applies_to(self, field: &str) -> bool {
        crate::sql::ColumnIdentifierPath::stored_field_key(self.field)
            == crate::sql::ColumnIdentifierPath::stored_field_key(field)
    }

    pub(crate) fn value_for(
        added: Option<Self>,
        object: &'a serde_json::Map<String, serde_json::Value>,
        field: &str,
    ) -> Option<&'a serde_json::Value> {
        added
            .filter(|default| default.applies_to(field))
            .map(|default| default.value)
            .or_else(|| object.get(field))
    }
}

impl Cassie {
    /// Validates existing rows against newly added CHECK and NOT NULL constraints.
    pub(crate) fn validate_existing_check_and_not_null_rows(
        &self,
        collection: &str,
        constraints: &[FieldConstraint],
    ) -> Result<(), CassieError> {
        self.validate_existing_rows_after_added_column(collection, constraints, None)
    }

    /// Validates existing rows against constraints, treating `added` as the
    /// value each row will hold for a column being added with a default.
    /// A single-column UNIQUE or PRIMARY KEY on that column fails when a
    /// non-null default would be backfilled into two or more rows.
    pub(crate) fn validate_existing_rows_after_added_column(
        &self,
        collection: &str,
        constraints: &[FieldConstraint],
        added: Option<AddedColumnDefault<'_>>,
    ) -> Result<(), CassieError> {
        crate::sql::definition_guard::constraints(constraints)?;
        if constraints.iter().all(|constraint| {
            !constraint.primary_key && !constraint.not_null && constraint.check.is_none()
        }) && added.is_none_or(|default| default.value.is_null())
        {
            return Ok(());
        }
        let schema = self.catalog.get_schema(collection);

        let mut row_count = 0_usize;
        for document in self
            .scan_documents_batched_for_session(None, collection, CONSTRAINT_SCAN_BATCH_SIZE)?
            .into_iter()
            .flatten()
        {
            let object = document.payload.as_object().ok_or_else(|| {
                CassieError::InvalidVector("document payload must be a JSON object".to_string())
            })?;
            row_count += 1;
            if row_count == 2 {
                Self::reject_duplicate_added_default(collection, constraints, added)?;
            }
            for constraint in constraints {
                let existing = AddedColumnDefault::value_for(added, object, &constraint.field);
                if (constraint.not_null || constraint.primary_key)
                    && super::field_value_is_sql_null(existing, schema.as_ref(), &constraint.field)
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

                if let Some(check) = &constraint.check {
                    if !Self::satisfies_column_check(schema.as_ref(), existing, check)? {
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

    /// Two rows backfilled with the same non-null default collide on a
    /// single-column UNIQUE or PRIMARY KEY constraint of the added column.
    fn reject_duplicate_added_default(
        collection: &str,
        constraints: &[FieldConstraint],
        added: Option<AddedColumnDefault<'_>>,
    ) -> Result<(), CassieError> {
        let Some(default) = added.filter(|default| !default.value.is_null()) else {
            return Ok(());
        };
        let Some(constraint) = constraints.iter().find(|constraint| {
            default.applies_to(&constraint.field)
                && crate::catalog::enforces_single_column_uniqueness(constraint, constraints)
        }) else {
            return Ok(());
        };
        Err(CassieError::UniqueViolation {
            table: collection.to_string(),
            column: constraint.field.clone(),
            constraint: constraint.unique_constraint_name(collection),
        })
    }
}
