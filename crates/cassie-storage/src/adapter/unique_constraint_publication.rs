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
        let previous = cassie_sql::attachment::rebind_attachment::<A, _>(
            self.load_constraints(collection)?,
        );
        let fields = constraints
            .iter()
            .filter(|constraint| {
                crate::catalog::enforces_single_column_uniqueness(constraint, constraints)
                    && !previous.iter().any(|existing| {
                        cassie_sql::column_identifier::ColumnIdentifierPath::stored_field_key(&existing.field)
                            == cassie_sql::column_identifier::ColumnIdentifierPath::stored_field_key(&constraint.field)
                            &&crate::catalog::enforces_single_column_uniqueness(
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
                                    cassie_core::types::DataType::Json | cassie_core::types::DataType::Jsonb
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
                                cassie_core::types::DataType::Json | cassie_core::types::DataType::Jsonb
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
    use crate::adapter::{key_encoding, StorageFamily};
    use cassie_core::types::{DataType, FieldSchema, Schema};

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

    #[test]
    fn should_preserve_pending_publication_bytes_for_non_serde_attachment_type() {
        // Arrange
        #[derive(Clone)]
        struct RuntimeAttachment;

        let mut storage_constraint = FieldConstraint::new("value");
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

    #[test]
    fn should_discard_unique_publication_record_when_its_table_no_longer_exists() {
        // Arrange
        let path = std::env::temp_dir().join(format!(
            "cassie-unique-publication-orphan-{}",
            uuid::Uuid::new_v4()
        ));
        let cassie =
            Cassie::new_with_data_dir_and_config(&path, CassieRuntimeConfig::default()).unwrap();
        cassie.startup().unwrap();
        let mut constraint = FieldConstraint::new("value");
        constraint.unique = true;
        let publication = PendingUniqueConstraintPublication {
            collection: CANONICAL.into(),
            constraints: vec![constraint.clone()],
            fields: vec![constraint],
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
        let started = restarted.startup();
        let pending = restarted
            .midge
            .raw_scan_prefix(
                StorageFamily::Data,
                &key_encoding::unique_constraint_publication_prefix(),
            )
            .unwrap();

        // Assert
        assert!(started.is_ok(), "startup must survive an orphaned record");
        assert!(pending.is_empty(), "orphaned record must be discarded");
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
