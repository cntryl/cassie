//! Persisted selected PostgreSQL object comments.

use super::{
    collect_scan, key_encoding, AdapterTransaction, CassieError, Midge, Query, TransactionMode,
};
use crate::catalog::{ObjectCommentAddress, ObjectCommentMeta};

impl Midge {
    /// Persist a non-empty comment in its owning database family.
    ///
    /// # Errors
    ///
    /// Returns an error when the address is unsupported or durable publication fails.
    pub(crate) fn put_object_comment(
        &self,
        metadata: &ObjectCommentMeta,
    ) -> Result<(), CassieError> {
        let mut metadata = metadata.clone();
        metadata.address.database.make_ascii_lowercase();
        metadata
            .validate(&metadata.address.database)
            .map_err(CassieError::Unsupported)?;
        let database = metadata.address.database.clone();
        let key = key_encoding::object_comment_key(
            metadata.address.class_oid,
            metadata.address.object_oid,
            metadata.address.object_sub_id,
        );
        let value = serde_json::to_vec(&metadata)
            .map_err(|error| CassieError::Parse(format!("serialize object comment: {error}")))?;
        let mut tx = self.begin_database_adapter_tx(&database, TransactionMode::ReadWrite)?;
        tx.put(key, value, None).map_err(CassieError::from)?;
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)
    }

    /// Persists LIKE column comments inside the target collection's initial transaction.
    pub(super) fn save_initial_column_comments_to_tx(
        tx: &mut AdapterTransaction,
        target: &str,
        schema: &crate::midge::row_blob::RowSchema,
        comments: &[(String, String)],
    ) -> Result<Vec<ObjectCommentMeta>, CassieError> {
        if comments.is_empty() {
            return Ok(Vec::new());
        }
        let database = crate::catalog::relation_database_name(target)
            .ok_or_else(|| CassieError::Execution("LIKE target has no database".to_owned()))?;
        let oid =
            crate::catalog::object_identity::normal_oid(schema.relation_id).ok_or_else(|| {
                CassieError::Execution("LIKE target has no normal object identity".to_owned())
            })?;
        let mut copied = Vec::new();
        for (name, description) in comments {
            let Some(field) = schema
                .fields
                .iter()
                .find(|field| !field.retired && field.name == *name)
            else {
                continue;
            };
            let sub_id = i32::try_from(field.field_id).map_err(|_| {
                CassieError::ResourceLimit(
                    "LIKE column comment field identity exceeds i32".to_owned(),
                )
            })?;
            let metadata = ObjectCommentMeta {
                format_version: ObjectCommentMeta::FORMAT_VERSION,
                address: ObjectCommentAddress {
                    database: database.clone(),
                    class_oid: 1259,
                    object_oid: oid,
                    object_sub_id: sub_id,
                },
                description: description.clone(),
            };
            metadata
                .validate(&database)
                .map_err(CassieError::Unsupported)?;
            let key = key_encoding::object_comment_key(1259, oid, sub_id);
            let value = serde_json::to_vec(&metadata).map_err(|error| {
                CassieError::Parse(format!("serialize LIKE column comment: {error}"))
            })?;
            tx.put(key, value, None).map_err(CassieError::from)?;
            copied.push(metadata);
        }
        Ok(copied)
    }

    /// Remove the addressed comment from its owning database family.
    ///
    /// # Errors
    ///
    /// Returns an error when the family cannot be written or publication fails.
    pub(crate) fn delete_object_comment(
        &self,
        address: &ObjectCommentAddress,
    ) -> Result<(), CassieError> {
        let database = address.database.to_ascii_lowercase();
        let key = key_encoding::object_comment_key(
            address.class_oid,
            address.object_oid,
            address.object_sub_id,
        );
        let mut tx = self.begin_database_adapter_tx(&database, TransactionMode::ReadWrite)?;
        tx.delete(key).map_err(CassieError::from)?;
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)
    }

    /// Remove every selected comment row for an object in its database family.
    ///
    /// # Errors
    ///
    /// Returns an error when the family cannot be written or publication fails.
    pub(crate) fn delete_object_comments_for_object(
        &self,
        database: &str,
        class_oid: u32,
        object_oid: u32,
    ) -> Result<(), CassieError> {
        let mut tx = self.begin_database_adapter_tx(database, TransactionMode::ReadWrite)?;
        Self::delete_object_comments_for_object_to_tx(&mut tx, class_oid, object_oid)?;
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)
    }

    /// Load and validate every selected comment record from all database families.
    ///
    /// # Errors
    ///
    /// Returns an error instead of hiding malformed or unsupported durable records.
    pub(crate) fn list_object_comments(&self) -> Result<Vec<ObjectCommentMeta>, CassieError> {
        let mut comments = Vec::new();
        let prefix = key_encoding::object_comment_prefix();
        for database in self.database_names() {
            let tx = self.begin_database_readonly_tx(&database)?;
            let entries = collect_scan(
                tx.scan(&Query::new().prefix(prefix.clone().into()))
                    .map_err(CassieError::from)?,
            )?;
            for (key, raw_value) in entries {
                let metadata: ObjectCommentMeta =
                    serde_json::from_slice(&raw_value).map_err(|error| {
                        CassieError::Parse(format!(
                            "invalid object comment metadata in database '{database}': {error}"
                        ))
                    })?;
                metadata.validate(&database).map_err(|error| {
                    CassieError::Parse(format!(
                        "invalid object comment metadata in database '{database}': {error}"
                    ))
                })?;
                let expected_key = key_encoding::object_comment_key(
                    metadata.address.class_oid,
                    metadata.address.object_oid,
                    metadata.address.object_sub_id,
                );
                if key != expected_key {
                    return Err(CassieError::Parse(format!(
                        "object comment key does not match its address in database '{database}'"
                    )));
                }
                comments.push(metadata);
            }
        }
        comments.sort_by_key(|metadata| {
            (
                metadata.address.database.to_ascii_lowercase(),
                metadata.address.class_oid,
                metadata.address.object_oid,
                metadata.address.object_sub_id,
            )
        });
        Ok(comments)
    }

    /// Delete all comments for one selected catalog object in an existing family transaction.
    pub(crate) fn delete_object_comments_for_object_to_tx(
        tx: &mut AdapterTransaction,
        class_oid: u32,
        object_oid: u32,
    ) -> Result<(), CassieError> {
        let prefix = key_encoding::object_comment_object_prefix(class_oid, object_oid);
        let entries = collect_scan(
            tx.scan(&Query::new().prefix(prefix.into()))
                .map_err(CassieError::from)?,
        )?;
        for (key, _) in entries {
            tx.delete(key).map_err(CassieError::from)?;
        }
        Ok(())
    }
}
