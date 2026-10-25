use serde::{Deserialize, Serialize};

use super::{key_encoding, CassieError, FieldConstraint, Midge, RowDecode};

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
        crate::sql::definition_guard::constraints(constraints)?;
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
                        crate::sql::ColumnIdentifierPath::stored_field_key(&existing.field)
                            == crate::sql::ColumnIdentifierPath::stored_field_key(&constraint.field)
                            && crate::catalog::enforces_single_column_uniqueness(
                                existing, &previous,
                            )
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
                            field_meta.name == constraint.field
                                && matches!(
                                    field_meta.data_type,
                                    crate::types::DataType::Json | crate::types::DataType::Jsonb
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
                        field_meta.name == constraint.field
                            && matches!(
                                field_meta.data_type,
                                crate::types::DataType::Json | crate::types::DataType::Jsonb
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

#[cfg(test)]
mod tests {
    use super::PendingUniqueConstraintPublication;
    use crate::app::{Cassie, CassieError};
    use crate::catalog::FieldConstraint;
    use crate::config::CassieRuntimeConfig;
    use crate::midge::adapter::{key_encoding, StorageFamily};
    use crate::types::{DataType, FieldSchema, Schema};

    const CANONICAL: &str = "postgres.public.unique_publication_recovery";

    #[test]
    fn should_replay_canonical_unique_publication_given_a_prepared_record_after_restart() {
        // Arrange
        let path = std::env::temp_dir().join(format!(
            "cassie-unique-publication-recovery-{}",
            uuid::Uuid::new_v4()
        ));
        let cassie =
            Cassie::new_with_data_dir_and_config(&path, CassieRuntimeConfig::default()).unwrap();
        cassie.startup().unwrap();
        cassie.midge.create_collection(CANONICAL, schema()).unwrap();
        cassie
            .midge
            .put_document(
                CANONICAL,
                Some("existing".into()),
                serde_json::json!({"value":"same"}),
            )
            .unwrap();
        let mut constraint = FieldConstraint::new("value");
        constraint.unique = true;
        constraint.unique_name = Some("recovered_unique".into());
        let publication = PendingUniqueConstraintPublication {
            collection: CANONICAL.into(),
            constraints: vec![constraint.clone()],
            fields: vec![constraint.clone()],
        };
        cassie
            .midge
            .raw_put(
                StorageFamily::Data,
                &key_encoding::unique_constraint_publication_key(CANONICAL),
                &serde_json::to_vec(&publication).unwrap(),
            )
            .unwrap();
        drop(cassie);

        // Act
        let restarted =
            Cassie::new_with_data_dir_and_config(&path, CassieRuntimeConfig::default()).unwrap();
        restarted.startup().unwrap();
        let metadata = restarted.midge.load_constraints(CANONICAL).unwrap();
        let duplicate = restarted.midge.put_document(
            CANONICAL,
            Some("duplicate".into()),
            serde_json::json!({"value":"same"}),
        );
        let pending = restarted
            .midge
            .raw_scan_prefix(
                StorageFamily::Data,
                &key_encoding::unique_constraint_publication_prefix(),
            )
            .unwrap();

        // Assert
        assert_eq!(metadata, vec![constraint]);
        assert!(
            matches!(&duplicate, Err(CassieError::UniqueViolation { constraint, .. }) if constraint == "recovered_unique"),
            "replayed publication must reject the duplicate: {duplicate:?}"
        );
        assert!(
            pending.is_empty(),
            "startup must retire the prepared record"
        );
        drop(restarted);
        std::fs::remove_dir_all(path).unwrap();
    }

    fn schema() -> Schema {
        Schema {
            fields: vec![FieldSchema {
                name: "value".into(),
                data_type: DataType::Text,
                nullable: true,
            }],
        }
    }
}
