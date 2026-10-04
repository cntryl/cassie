use super::{
    key_encoding, CassieError, DocumentWriteBatchContext, FieldConstraint, IndexMeta, Midge,
    PreparedWrite, RowSchema,
};

#[derive(Debug)]
enum UniqueReservationDescriptor {
    UniqueConstraint {
        table: String,
        field: String,
        constraint: String,
    },
    UniqueIndex(Box<CassieError>),
}

impl Midge {
    pub(super) fn release_unique_reservations_for_batch(
        tx: &mut cntryl_midge::Transaction,
        collection: &str,
        context: &DocumentWriteBatchContext,
        prepared: &[PreparedWrite],
    ) -> Result<(), CassieError> {
        if prepared.len() < 2
            || (context.unique_constraints.is_empty() && context.unique_scalar_indexes.is_empty())
        {
            return Ok(());
        }

        for write in prepared {
            let existing = Self::existing_document_state_for_prepared_write(
                tx, collection, context, &write.id,
            )?;
            Self::sync_unique_reservations_for_document(
                tx,
                collection,
                context,
                &write.id,
                existing.payload.as_ref(),
                None,
            )?;
        }
        Ok(())
    }

    pub(super) fn sync_unique_reservations_for_document(
        tx: &mut cntryl_midge::Transaction,
        collection: &str,
        context: &DocumentWriteBatchContext,
        id: &str,
        previous_payload: Option<&serde_json::Value>,
        next_payload: Option<&serde_json::Value>,
    ) -> Result<(), CassieError> {
        let owner = id.as_bytes();
        let mut stale_targets = Self::collect_unique_reservation_targets(
            collection,
            &context.unique_constraints,
            &context.unique_scalar_indexes,
            &context.row_schema,
            previous_payload,
        )?;
        let next_targets = Self::collect_unique_reservation_targets(
            collection,
            &context.unique_constraints,
            &context.unique_scalar_indexes,
            &context.row_schema,
            next_payload,
        )?;

        for (key, _descriptor) in stale_targets.drain(..) {
            if !Self::unique_reservation_targets_contains(&key, &next_targets) {
                let current_owner = tx.get(&key).map_err(CassieError::from)?;
                if current_owner.as_deref() == Some(owner) {
                    tx.delete(key).map_err(CassieError::from)?;
                }
            }
        }

        for (key, descriptor) in next_targets {
            if let Some(current_owner) = tx.get(&key).map_err(CassieError::from)? {
                if current_owner != owner {
                    return Err(match descriptor {
                        UniqueReservationDescriptor::UniqueConstraint {
                            table,
                            field,
                            constraint,
                        } => CassieError::UniqueViolation {
                            table,
                            column: field,
                            constraint,
                        },
                        UniqueReservationDescriptor::UniqueIndex(violation) => *violation,
                    });
                }
                continue;
            }
            tx.put(key, owner.to_vec(), None)
                .map_err(CassieError::from)?;
        }

        Ok(())
    }

    fn collect_unique_reservation_targets(
        collection: &str,
        constraints: &[FieldConstraint],
        unique_indexes: &[IndexMeta],
        row_schema: &RowSchema,
        payload: Option<&serde_json::Value>,
    ) -> Result<Vec<(Vec<u8>, UniqueReservationDescriptor)>, CassieError> {
        let Some(payload) = payload else {
            return Ok(Vec::new());
        };

        let mut targets = Vec::new();
        for constraint in constraints {
            let Some(value) = payload.get(&constraint.field) else {
                continue;
            };
            if value.is_null()
                && !row_schema.fields.iter().any(|field_meta| {
                    field_meta.name == constraint.field
                        && matches!(field_meta.data_type, crate::types::DataType::Json)
                })
            {
                continue;
            }

            let key = key_encoding::unique_constraint_reservation_key(
                collection,
                &constraint.field,
                value,
            )?;
            targets.push((
                key,
                UniqueReservationDescriptor::UniqueConstraint {
                    table: collection.to_string(),
                    field: constraint.field.clone(),
                    constraint: constraint.unique_constraint_name(collection),
                },
            ));
        }

        for index in unique_indexes {
            if !Self::payload_matches_scalar_index_predicate(index, payload, row_schema)? {
                continue;
            }
            let Some(values) = Self::scalar_index_key_values(index, payload, row_schema)? else {
                continue;
            };
            let key = key_encoding::unique_scalar_index_reservation_key(
                collection,
                &index.name,
                &values,
            )?;
            targets.push((
                key,
                UniqueReservationDescriptor::UniqueIndex(Box::new(
                    index.unique_violation(collection),
                )),
            ));
        }

        Ok(targets)
    }

    fn unique_reservation_targets_contains(
        candidate: &[u8],
        targets: &[(Vec<u8>, UniqueReservationDescriptor)],
    ) -> bool {
        targets.iter().any(|(key, _)| key == candidate)
    }
}
