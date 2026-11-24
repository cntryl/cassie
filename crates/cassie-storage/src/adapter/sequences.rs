use super::{key_encoding, CassieError, Midge};

impl Midge {
    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn put_sequence(&self, metadata: &crate::catalog::SequenceMeta) -> Result<(), CassieError> {
        let mut metadata = metadata.clone();
        metadata.name = self.canonical_collection_name(&metadata.name);
        let mut tx = self.begin_data_rw_tx_for(&metadata.name)?;
        let value =
            serde_json::to_vec(&metadata).map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(key_encoding::sequence_key(&metadata.name), value, None)
            .map_err(CassieError::from)?;
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn get_sequence(
        &self,
        name: &str,
    ) -> Result<Option<crate::catalog::SequenceMeta>, CassieError> {
        let canonical = self.canonical_collection_name(name);
        let tx = self.begin_data_readonly_tx_for(&canonical)?;
        let raw = tx
            .get(&key_encoding::sequence_key(&canonical))
            .map_err(CassieError::from)?;
        let Some(raw) = raw else {
            return Ok(self
                .list_sequences()?
                .into_iter()
                .find(|sequence| self.canonical_collection_name(&sequence.name) == canonical));
        };
        serde_json::from_slice(&raw)
            .map(Some)
            .map_err(|error| CassieError::Parse(format!("invalid sequence metadata: {error}")))
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn list_sequences(&self) -> Result<Vec<crate::catalog::SequenceMeta>, CassieError> {
        let entries = self.scan_database_families(&key_encoding::sequence_prefix())?;
        let mut out = Vec::with_capacity(entries.len());
        for (_key, raw_value) in entries {
            let Ok(record) = serde_json::from_slice(&raw_value) else {
                continue;
            };
            out.push(record);
        }
        out.sort_by_key(|sequence: &crate::catalog::SequenceMeta| {
            sequence.name.to_ascii_lowercase()
        });
        Ok(out)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn delete_sequence(&self, name: &str) -> Result<(), CassieError> {
        let stored_name = self.get_sequence(name)?.map_or_else(
            || self.canonical_collection_name(name),
            |sequence| sequence.name,
        );
        let mut tx = self.begin_data_rw_tx_for(&stored_name)?;
        tx.delete(key_encoding::sequence_key(&stored_name))
            .map_err(CassieError::from)?;
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    /// `nextval`: advances the stored sequence atomically.
    ///
    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn next_sequence_value(&self, name: &str) -> Result<i64, CassieError> {
        self.update_sequence_retrying(name, |metadata| {
            metadata.advance().map_err(CassieError::from)
        })
        .map(|(value, _)| value)
    }

    /// Like [`Self::update_sequence`], but retries when a concurrent update to
    /// the same sequence wins the commit, so SQL callers never observe the
    /// storage write conflict that PostgreSQL's `nextval` cannot raise. A
    /// fenced writer returns its error immediately without another attempt.
    ///
    /// # Errors
    ///
    /// Returns the update's error, or a non-retryable storage error.
    pub fn update_sequence_retrying<T>(
        &self,
        name: &str,
        mut update: impl FnMut(&mut crate::catalog::SequenceMeta) -> Result<T, CassieError>,
    ) -> Result<(T, crate::catalog::SequenceMeta), CassieError> {
        const MAX_ATTEMPTS: usize = 64;
        match self.private_sequence_target(name)? {
            super::transactions::sequence_owner::SequenceTarget::Private {
                metadata,
                owner,
                overlay,
                epoch,
                scratch: _scratch,
            } => return self.advance_private_sequence(&metadata, &owner, &overlay, epoch, update),
            super::transactions::sequence_owner::SequenceTarget::Native => {}
        }
        let mut attempt = 1;
        loop {
            match self.update_sequence(name, &mut update) {
                Err(error @ CassieError::StorageRetryable(_))
                    if attempt < MAX_ATTEMPTS
                        && error.storage_retry_kind()
                            != Some(crate::app::StorageRetryKind::Fenced) =>
                {
                    attempt += 1;
                }
                outcome => return outcome,
            }
        }
    }

    /// Applies `update` to the stored sequence in one database-family transaction and
    /// returns its result with the updated metadata.
    ///
    /// # Errors
    ///
    /// Returns the update's error, or a storage error.
    pub fn update_sequence<T>(
        &self,
        name: &str,
        update: impl FnOnce(&mut crate::catalog::SequenceMeta) -> Result<T, CassieError>,
    ) -> Result<(T, crate::catalog::SequenceMeta), CassieError> {
        let update = if self.private_sequence_catalog_scope() {
            match self.private_sequence_target(name)? {
                super::transactions::sequence_owner::SequenceTarget::Private {
                    metadata,
                    epoch,
                    scratch: _scratch,
                    ..
                } => {
                    return self.update_private_sequence_transactionally(
                        name,
                        metadata.object_id,
                        epoch,
                        update,
                    )
                }
                super::transactions::sequence_owner::SequenceTarget::Native => update,
            }
        } else {
            update
        };
        let stored_name = self
            .get_sequence(name)?
            .ok_or_else(|| CassieError::CatalogObjectNotFound {
                kind: crate::app::CatalogObjectKind::Sequence,
                name: name.to_string(),
            })?
            .name;
        let mut tx = self.begin_data_rw_tx_for(&stored_name)?;
        let raw = tx
            .get(&key_encoding::sequence_key(&stored_name))
            .map_err(CassieError::from)?
            .ok_or_else(|| CassieError::CatalogObjectNotFound {
                kind: crate::app::CatalogObjectKind::Sequence,
                name: name.to_string(),
            })?;
        let mut metadata: crate::catalog::SequenceMeta =
            serde_json::from_slice(&raw).map_err(|error| {
                CassieError::Parse(format!("invalid sequence metadata for '{name}': {error}"))
            })?;
        let result = update(&mut metadata)?;
        let value =
            serde_json::to_vec(&metadata).map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(key_encoding::sequence_key(&stored_name), value, None)
            .map_err(CassieError::from)?;
        super::schema_write_control::pause_before_schema_write_commit(
            super::schema_write_control::SchemaWritePausePoint::SequenceNextValue,
        );
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)?;
        Ok((result, metadata))
    }
}

#[cfg(test)]
mod retry_tests {
    use super::Midge;
    use crate::app::{CassieError, RetryableStorageError, StorageRetryKind};
    use crate::catalog::SequenceMeta;
    use crate::types::DataType;

    fn fixture() -> Midge {
        let midge = Midge::new_in_memory("postgres").expect("memory Midge");
        midge
            .put_sequence(&SequenceMeta::new("sequence_retry", DataType::BigInt))
            .expect("create sequence");
        midge
    }

    #[test]
    fn should_return_simulated_fenced_sequence_error_after_one_attempt() {
        // Arrange
        let midge = fixture();
        let before = midge.get_sequence("sequence_retry").expect("read");
        let mut attempts = 0;

        // Act: the callback simulates the typed error; no native fencing is injected.
        let result = midge.update_sequence_retrying("sequence_retry", |metadata| {
            attempts += 1;
            metadata.advance().expect("advance tentative metadata");
            Err::<i64, _>(CassieError::from(cntryl_midge::MidgeError::Fenced(
                "simulated stale writer".to_owned(),
            )))
        });

        // Assert
        let error = result.expect_err("fenced update must fail");
        assert_eq!(error.storage_retry_kind(), Some(StorageRetryKind::Fenced));
        assert!(error.to_string().contains("simulated stale writer"));
        assert_eq!(attempts, 1);
        assert_eq!(midge.get_sequence("sequence_retry").expect("read"), before);
    }

    #[test]
    fn should_retry_simulated_sequence_write_conflict() {
        // Arrange
        let midge = fixture();
        let mut attempts = 0;

        // Act
        let (value, _) = midge
            .update_sequence_retrying("sequence_retry", |metadata| {
                attempts += 1;
                if attempts == 1 {
                    return Err(CassieError::from(cntryl_midge::MidgeError::WriteConflict(
                        "simulated conflict".to_owned(),
                    )));
                }
                metadata.advance().map_err(Into::into)
            })
            .expect("retry conflict");

        // Assert
        assert_eq!(attempts, 2);
        assert_eq!(value, 1);
        let stored = midge
            .get_sequence("sequence_retry")
            .expect("read")
            .expect("sequence");
        assert_eq!(stored.current_value, 1);
        assert!(stored.is_called);
    }

    #[test]
    fn should_return_simulated_unknown_sequence_outcome_without_retry() {
        // Arrange
        let midge = fixture();
        let before = midge.get_sequence("sequence_retry").expect("read");
        let mut attempts = 0;

        // Act: this is a callback error, not an injected native commit outcome.
        let result = midge.update_sequence_retrying("sequence_retry", |metadata| {
            attempts += 1;
            metadata.advance().expect("advance tentative metadata");
            Err::<i64, _>(CassieError::from(cntryl_midge::MidgeError::OutcomeUnknown(
                "simulated unknown".to_owned(),
            )))
        });

        // Assert
        assert!(
            matches!(result, Err(CassieError::Storage(message)) if message.contains("simulated unknown"))
        );
        assert_eq!(attempts, 1);
        assert_eq!(midge.get_sequence("sequence_retry").expect("read"), before);
    }

    #[test]
    fn should_preserve_other_sequence_retryable_classifications() {
        // Arrange
        for kind in [StorageRetryKind::WriteStall, StorageRetryKind::Unavailable] {
            let midge = fixture();
            let mut attempts = 0;

            // Act
            let (value, _) = midge
                .update_sequence_retrying("sequence_retry", |metadata| {
                    attempts += 1;
                    if attempts == 1 {
                        return Err(CassieError::StorageRetryable(RetryableStorageError::new(
                            kind,
                            "simulated temporary error".to_owned(),
                        )));
                    }
                    metadata.advance().map_err(Into::into)
                })
                .expect("retry existing classification");

            // Assert
            assert_eq!(attempts, 2);
            assert_eq!(value, 1);
        }
    }
}
