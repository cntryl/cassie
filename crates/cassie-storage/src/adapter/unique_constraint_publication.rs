use serde::{Deserialize, Serialize};

use super::{key_encoding, CassieError, FieldConstraint, Midge, RowDecode};

const UNIQUE_BACKFILL_BATCH_SIZE: usize = 5_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound(serialize = "", deserialize = ""))]
struct PendingUniqueConstraintPublication<A = ()> {
    collection: String,
    constraints: Vec<FieldConstraint<A>>,
    fields: Vec<FieldConstraint<A>>,
}

impl Midge {
    /// Saves constraints and backfills reservations for newly unique fields before publishing them.
    ///
    /// # Errors
    ///
    /// Returns an error if existing rows violate uniqueness or storage work fails.
    pub fn save_constraints_with_unique_reservations<A: Clone>(
        &self,
        collection: &str,
        constraints: &[FieldConstraint<A>],
    ) -> Result<(), CassieError> {
        self.require_constraint_definitions(constraints)?;
        let collection = self.canonical_collection_name(collection);
        self.with_collection_write_gates(std::slice::from_ref(&collection), || {
            self.prepare_unique_constraint_publication(&collection, constraints)
        })
    }

    /// Replays durable unique-constraint backfills before the database begins serving writes.
    ///
    /// # Errors
    ///
    /// Returns an error when a pending publication is invalid or cannot be completed.
    pub fn replay_pending_unique_constraint_publications(&self) -> Result<(), CassieError> {
        let entries =
            self.scan_database_families(&key_encoding::unique_constraint_publication_prefix())?;
        let pending = entries
            .into_iter()
            .map(|(_, raw)| {
                serde_json::from_slice::<PendingUniqueConstraintPublication>(&raw).map_err(
                    |error| {
                        CassieError::Parse(format!(
                            "invalid unique constraint publication: {error}"
                        ))
                    },
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        for publication in pending {
            let collection = publication.collection.clone();
            let published = self
                .with_collection_write_gates(std::slice::from_ref(&collection), || {
                    self.publish_unique_constraint_publication(&publication)
                });
            match published {
                Err(
                    error @ (CassieError::UniqueViolation { .. }
                    | CassieError::CollectionNotFound(_)),
                ) => {
                    // Retrying cannot succeed: the table is gone or its rows violate
                    // the constraint, so the record would only block every restart.
                    self.discard_unique_constraint_publication(&collection)?;
                    tracing::warn!(
                        %collection,
                        %error,
                        "discarded a pending unique constraint publication that can no longer complete"
                    );
                }
                other => other?,
            }
        }
        Ok(())
    }

    fn discard_unique_constraint_publication(&self, collection: &str) -> Result<(), CassieError> {
        let mut tx = self.begin_data_rw_tx_for(collection)?;
        tx.delete(key_encoding::unique_constraint_publication_key(collection))
            .map_err(CassieError::from)?;
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)
    }

    fn prepare_unique_constraint_publication<A: Clone>(
        &self,
        collection: &str,
        constraints: &[FieldConstraint<A>],
    ) -> Result<(), CassieError> {
        let previous =
            cassie_sql::attachment::rebind_attachment::<A, _>(self.load_constraints(collection)?);
        let fields = constraints
            .iter()
            .filter(|constraint| {
                crate::catalog::enforces_single_column_uniqueness(constraint, constraints)
                    && !previous.iter().any(|existing| {
                        cassie_sql::column_identifier::ColumnIdentifierPath::stored_field_key(
                            &existing.field,
                        ) == cassie_sql::column_identifier::ColumnIdentifierPath::stored_field_key(
                            &constraint.field,
                        ) && crate::catalog::enforces_single_column_uniqueness(existing, &previous)
                    })
            })
            .cloned()
            .collect::<Vec<_>>();
        if fields.is_empty() {
            return self.save_constraints_metadata(collection, constraints);
        }

        let publication = PendingUniqueConstraintPublication {
            collection: collection.to_string(),
            constraints: constraints.to_vec(),
            fields,
        };
        self.validate_unique_constraint_rows(&publication)?;
        let mut tx = self.begin_data_rw_tx_for(collection)?;
        tx.put(
            key_encoding::unique_constraint_publication_key(collection),
            serde_json::to_vec(&publication)
                .map_err(|error| CassieError::Parse(error.to_string()))?,
            None,
        )
        .map_err(CassieError::from)?;
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)?;
        self.publish_unique_constraint_publication(&publication)
    }

    fn publish_unique_constraint_publication<A>(
        &self,
        publication: &PendingUniqueConstraintPublication<A>,
    ) -> Result<(), CassieError> {
        self.validate_unique_constraint_rows(publication)?;
        let row_schema = self.row_schema(&publication.collection)?;
        let rows = self.scan_rows_for_rebuild(&publication.collection, RowDecode::Full)?;
        for batch in rows.chunks(UNIQUE_BACKFILL_BATCH_SIZE) {
            let mut tx = self.begin_data_rw_tx_for(&publication.collection)?;
            for row in batch {
                for constraint in &publication.fields {
                    let Some(value) = row.payload.get(&constraint.field) else {
                        continue;
                    };
                    if value.is_null()
                        && !row_schema.fields.iter().any(|field_meta| {
                            field_meta.name == constraint.field
                                && matches!(
                                    field_meta.data_type,
                                    cassie_core::types::DataType::Json
                                        | cassie_core::types::DataType::Jsonb
                                )
                        })
                    {
                        continue;
                    }
                    let key = key_encoding::unique_constraint_reservation_key(
                        &publication.collection,
                        &constraint.field,
                        value,
                    )?;
                    tx.put(key, row.id.as_bytes().to_vec(), None)
                        .map_err(CassieError::from)?;
                }
            }
            tx.commit(self.write_options_sync())
                .map_err(CassieError::from)?;
        }

        let mut tx = self.begin_data_rw_tx_for(&publication.collection)?;
        tx.put(
            Self::constraints_key(&publication.collection),
            serde_json::to_vec(&publication.constraints)
                .map_err(|error| CassieError::Parse(error.to_string()))?,
            None,
        )
        .map_err(CassieError::from)?;
        tx.delete(key_encoding::unique_constraint_publication_key(
            &publication.collection,
        ))
        .map_err(CassieError::from)?;
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)
    }

    fn validate_unique_constraint_rows<A>(
        &self,
        publication: &PendingUniqueConstraintPublication<A>,
    ) -> Result<(), CassieError> {
        let rows = self.scan_rows_for_rebuild(&publication.collection, RowDecode::Full)?;
        let row_schema = self.row_schema(&publication.collection)?;
        for constraint in &publication.fields {
            let mut owners = std::collections::HashMap::<Vec<u8>, &str>::new();
            for row in &rows {
                let Some(value) = row.payload.get(&constraint.field) else {
                    continue;
                };
                if value.is_null()
                    && !row_schema.fields.iter().any(|field_meta| {
                        field_meta.name == constraint.field
                            && matches!(
                                field_meta.data_type,
                                cassie_core::types::DataType::Json
                                    | cassie_core::types::DataType::Jsonb
                            )
                    })
                {
                    continue;
                }
                let key = key_encoding::unique_constraint_reservation_key(
                    &publication.collection,
                    &constraint.field,
                    value,
                )?;
                if owners.insert(key, &row.id).is_some() {
                    return Err(CassieError::UniqueViolation {
                        table: publication.collection.clone(),
                        column: constraint.field.clone(),
                        constraint: constraint.unique_constraint_name(&publication.collection),
                    });
                }
            }
        }
        Ok(())
    }
}

#[cfg(feature = "storage-test-hooks")]
impl Midge {
    /// Writes a prepared uniqueness record for startup recovery fixtures.
    ///
    /// # Errors
    /// Returns an error if the record cannot be encoded or persisted.
    #[doc(hidden)]
    pub fn seed_pending_unique_constraint_publication(
        &self,
        collection: &str,
        constraints: Vec<FieldConstraint>,
        fields: Vec<FieldConstraint>,
    ) -> Result<(), CassieError> {
        let publication = PendingUniqueConstraintPublication {
            collection: collection.to_owned(),
            constraints,
            fields,
        };
        let raw = serde_json::to_vec(&publication)?;
        self.raw_put(
            super::StorageFamily::Data,
            &key_encoding::unique_constraint_publication_key(collection),
            &raw,
        )
    }

    /// Returns the number of prepared uniqueness records in the test database.
    ///
    /// # Errors
    /// Returns an error when native storage cannot read the journal.
    #[doc(hidden)]
    pub fn pending_unique_constraint_publication_count(&self) -> Result<usize, CassieError> {
        self.raw_scan_prefix(
            super::StorageFamily::Data,
            &key_encoding::unique_constraint_publication_prefix(),
        )
        .map(|records| records.len())
    }
}

#[cfg(test)]
mod tests {
    use super::{FieldConstraint, PendingUniqueConstraintPublication};

    const CANONICAL: &str = "postgres.public.unique_publication_recovery";

    #[test]
    fn should_preserve_pending_publication_bytes_for_non_serde_attachment_type() {
        // Arrange
        #[derive(Clone)]
        struct RuntimeAttachment;

        let mut storage_constraint = FieldConstraint::<()>::new("value");
        storage_constraint.unique = true;
        storage_constraint.unique_name = Some("value_unique".into());
        storage_constraint.check = Some(cassie_sql::declarations::ConstraintCheck {
            field: "value".into(),
            operator: cassie_sql::declarations::ConstraintOperator::Eq,
            value: serde_json::Value::from(1),
            bound_expression: Some(Box::new(cassie_sql::ast::Expr::IntegerLiteral(1))),
        });
        let runtime_constraint =
            cassie_sql::declarations::FieldConstraint::<RuntimeAttachment>::new("value");
        let mut runtime_constraint = runtime_constraint;
        runtime_constraint.unique = true;
        runtime_constraint.unique_name = Some("value_unique".into());
        runtime_constraint.check = Some(cassie_sql::declarations::ConstraintCheck {
            field: "value".into(),
            operator: cassie_sql::declarations::ConstraintOperator::Eq,
            value: serde_json::Value::from(1),
            bound_expression: Some(Box::new(cassie_sql::ast::Expr::IntegerLiteral(1))),
        });

        let storage_publication = PendingUniqueConstraintPublication {
            collection: CANONICAL.into(),
            constraints: vec![storage_constraint.clone()],
            fields: vec![storage_constraint],
        };
        let runtime_publication = PendingUniqueConstraintPublication {
            collection: CANONICAL.into(),
            constraints: vec![runtime_constraint.clone()],
            fields: vec![runtime_constraint],
        };

        // Act
        let storage_bytes = serde_json::to_vec(&storage_publication).unwrap();
        let runtime_bytes = serde_json::to_vec(&runtime_publication).unwrap();

        // Assert
        assert_eq!(runtime_bytes, storage_bytes);
    }
}
