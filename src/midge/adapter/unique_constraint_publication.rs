use serde::{Deserialize, Serialize};

use super::{key_encoding, CassieError, FieldConstraint, Midge, Query, RowDecode};

const UNIQUE_BACKFILL_BATCH_SIZE: usize = 5_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingUniqueConstraintPublication {
    collection: String,
    constraints: Vec<FieldConstraint>,
    fields: Vec<FieldConstraint>,
}

impl Midge {
    /// Saves constraints and backfills reservations for newly unique fields before publishing them.
    ///
    /// # Errors
    ///
    /// Returns an error if existing rows violate uniqueness or storage work fails.
    pub fn save_constraints_with_unique_reservations(
        &self,
        collection: &str,
        constraints: &[FieldConstraint],
    ) -> Result<(), CassieError> {
        let collection = collection.to_string();
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
        let tx = self.begin_schema_readonly_tx()?;
        let entries = super::collect_scan(
            tx.scan(
                &Query::new().prefix(key_encoding::unique_constraint_publication_prefix().into()),
            )
            .map_err(CassieError::from)?,
        )?;
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
            self.with_collection_write_gates(std::slice::from_ref(&collection), || {
                self.publish_unique_constraint_publication(&publication)
            })?;
        }
        Ok(())
    }

    fn prepare_unique_constraint_publication(
        &self,
        collection: &str,
        constraints: &[FieldConstraint],
    ) -> Result<(), CassieError> {
        let previous = self.load_constraints(collection)?;
        let fields = constraints
            .iter()
            .filter(|constraint| {
                crate::catalog::enforces_single_column_uniqueness(constraint, constraints)
                    && !previous.iter().any(|existing| {
                        existing.field.eq_ignore_ascii_case(&constraint.field)
                            && crate::catalog::enforces_single_column_uniqueness(
                                existing, &previous,
                            )
                    })
            })
            .cloned()
            .collect::<Vec<_>>();
        if fields.is_empty() {
            return self.save_constraints(collection, constraints);
        }

        let publication = PendingUniqueConstraintPublication {
            collection: collection.to_string(),
            constraints: constraints.to_vec(),
            fields,
        };
        self.validate_unique_constraint_rows(&publication)?;
        let mut tx = self.begin_schema_rw_tx()?;
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

    fn publish_unique_constraint_publication(
        &self,
        publication: &PendingUniqueConstraintPublication,
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
                            field_meta.name.eq_ignore_ascii_case(&constraint.field)
                                && matches!(field_meta.data_type, crate::types::DataType::Json)
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

        let mut tx = self.begin_schema_rw_tx()?;
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

    fn validate_unique_constraint_rows(
        &self,
        publication: &PendingUniqueConstraintPublication,
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
                        field_meta.name.eq_ignore_ascii_case(&constraint.field)
                            && matches!(field_meta.data_type, crate::types::DataType::Json)
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
