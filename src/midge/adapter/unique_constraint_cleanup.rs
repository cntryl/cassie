use serde::{Deserialize, Serialize};

use super::{
    check_unique_constraint_cleanup_failure_point, key_encoding, CassieError, FieldConstraint,
    Midge, Query,
};

const UNIQUE_RESERVATION_CLEANUP_BATCH_SIZE: usize = 5_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingUniqueConstraintCleanup {
    collection: String,
    fields: Vec<String>,
}

impl Midge {
    /// Saves the constraint set and durably schedules reservation cleanup for dropped fields.
    ///
    /// # Errors
    ///
    /// Returns an error when schema metadata or reservation cleanup cannot be persisted.
    pub fn save_constraints_and_release_unique_reservations(
        &self,
        collection: &str,
        constraints: &[FieldConstraint],
        fields: &[String],
    ) -> Result<(), CassieError> {
        let collection = collection.to_string();
        let mut fields = fields.to_vec();
        fields.sort_by_key(|field| field.to_ascii_lowercase());
        fields.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
        self.with_collection_write_gates(std::slice::from_ref(&collection), || {
            if fields.is_empty() {
                return self.save_constraints(&collection, constraints);
            }
            let cleanup = PendingUniqueConstraintCleanup {
                collection: collection.clone(),
                fields,
            };
            let mut tx = self.begin_schema_rw_tx()?;
            tx.put(
                Self::constraints_key(&collection),
                serde_json::to_vec(constraints)
                    .map_err(|error| CassieError::Parse(error.to_string()))?,
                None,
            )
            .map_err(CassieError::from)?;
            tx.put(
                key_encoding::unique_constraint_cleanup_key(&collection),
                serde_json::to_vec(&cleanup)
                    .map_err(|error| CassieError::Parse(error.to_string()))?,
                None,
            )
            .map_err(CassieError::from)?;
            tx.commit(self.write_options_sync())
                .map_err(CassieError::from)?;
            check_unique_constraint_cleanup_failure_point()?;
            self.complete_unique_constraint_cleanup(&cleanup)
        })
    }

    /// Replays durable unique-reservation cleanup before the database accepts writes.
    ///
    /// # Errors
    ///
    /// Returns an error when a pending cleanup is invalid or cannot be completed.
    pub fn replay_pending_unique_constraint_cleanups(&self) -> Result<(), CassieError> {
        let tx = self.begin_schema_readonly_tx()?;
        let entries = super::collect_scan(
            tx.scan(&Query::new().prefix(key_encoding::unique_constraint_cleanup_prefix().into()))
                .map_err(CassieError::from)?,
        )?;
        let mut cleanups = entries
            .into_iter()
            .map(|(_, raw)| {
                serde_json::from_slice::<PendingUniqueConstraintCleanup>(&raw).map_err(|error| {
                    CassieError::Parse(format!("invalid unique constraint cleanup: {error}"))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        cleanups.sort_by(|left, right| left.collection.cmp(&right.collection));
        for cleanup in cleanups {
            let collection = cleanup.collection.clone();
            self.with_collection_write_gates(std::slice::from_ref(&collection), || {
                self.complete_unique_constraint_cleanup(&cleanup)
            })?;
        }
        Ok(())
    }

    fn complete_unique_constraint_cleanup(
        &self,
        cleanup: &PendingUniqueConstraintCleanup,
    ) -> Result<(), CassieError> {
        for field in &cleanup.fields {
            let prefix = key_encoding::unique_constraint_reservation_field_prefix(
                &cleanup.collection,
                field,
            );
            loop {
                let entries = self.raw_scan_prefix_page_for_collection(
                    &cleanup.collection,
                    &prefix,
                    UNIQUE_RESERVATION_CLEANUP_BATCH_SIZE,
                )?;
                if entries.is_empty() {
                    break;
                }
                let mut tx = self.begin_data_rw_tx_for(&cleanup.collection)?;
                for (key, _) in entries {
                    tx.delete(key).map_err(CassieError::from)?;
                }
                tx.commit(self.write_options_sync())
                    .map_err(CassieError::from)?;
            }
        }

        let mut tx = self.begin_schema_rw_tx()?;
        tx.delete(key_encoding::unique_constraint_cleanup_key(
            &cleanup.collection,
        ))
        .map_err(CassieError::from)?;
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)
    }
}
