//! Durable direct schema and table grants in the shared Schema family.

use std::collections::BTreeSet;

use super::{collect_scan, key_encoding, AdapterTransaction, CassieError, Midge, Query};
use crate::catalog::{RoleGrantTarget, RoleObjectGrantMeta, RoleObjectPrivilege};

impl Midge {
    /// Add or remove selected direct privileges for a stable database object.
    ///
    /// # Errors
    ///
    /// Returns an error for unsupported target/privilege combinations, malformed durable
    /// records, or a failed Schema-family transaction.
    pub fn update_role_object_grant(
        &self,
        grant: &RoleObjectGrantMeta,
        privileges: &[RoleObjectPrivilege],
        enabled: bool,
    ) -> Result<Option<RoleObjectGrantMeta>, CassieError> {
        validate_identity(grant)?;
        validate_privileges(&grant.target, privileges)?;
        let key = key_encoding::role_object_grant_key(grant);
        let mut tx = self.begin_schema_rw_tx()?;
        let mut updated = if let Some(raw) = tx.get(&key).map_err(CassieError::from)? {
            decode_record(&key, &raw)?
        } else {
            RoleObjectGrantMeta {
                database_family: grant.database_family.clone(),
                role_id: grant.role_id,
                target: grant.target.clone(),
                privileges: BTreeSet::new(),
            }
        };
        for privilege in privileges {
            if enabled {
                updated.privileges.insert(*privilege);
            } else {
                updated.privileges.remove(privilege);
            }
        }
        if updated.privileges.is_empty() {
            tx.delete(key).map_err(CassieError::from)?;
            tx.commit(self.write_options_sync())
                .map_err(CassieError::from)?;
            return Ok(None);
        }
        validate_record(&updated)?;
        let value = serde_json::to_vec(&updated)
            .map_err(|error| CassieError::Parse(format!("serialize role object grant: {error}")))?;
        tx.put(key, value, None).map_err(CassieError::from)?;
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)?;
        Ok(Some(updated))
    }

    /// Load every direct grant and reject malformed or mismatched durable records.
    ///
    /// # Errors
    ///
    /// Returns an error when storage or a record is malformed.
    pub fn list_role_object_grants(&self) -> Result<Vec<RoleObjectGrantMeta>, CassieError> {
        let tx = self.begin_schema_readonly_tx()?;
        let prefix = key_encoding::role_object_grant_prefix();
        let entries = collect_scan(
            tx.scan(&Query::new().prefix(prefix.into()))
                .map_err(CassieError::from)?,
        )?;
        let mut grants = entries
            .iter()
            .map(|(key, value)| decode_record(key, value))
            .collect::<Result<Vec<_>, _>>()?;
        grants.sort_by_key(RoleObjectGrantMeta::catalog_key);
        Ok(grants)
    }

    /// Remove grants for selected stable targets in one Schema-family transaction.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid database family/target or malformed durable records.
    pub(crate) fn delete_role_object_grants_for_targets(
        &self,
        database_family: &str,
        targets: &[RoleGrantTarget],
    ) -> Result<Vec<RoleObjectGrantMeta>, CassieError> {
        if database_family.is_empty() {
            return Err(CassieError::Unsupported(
                "role object grant database family cannot be empty".to_owned(),
            ));
        }
        for target in targets {
            validate_target_identity(target)?;
        }
        if targets.is_empty() {
            return Ok(Vec::new());
        }
        let mut tx = self.begin_schema_rw_tx()?;
        let removed =
            Self::delete_role_object_grants_for_targets_to_tx(&mut tx, database_family, targets)?;
        if removed.is_empty() {
            return Ok(removed);
        }
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)?;
        Ok(removed)
    }

    /// Remove every grant record for a role across all database families.
    ///
    /// # Errors
    ///
    /// Returns an error when storage or a durable record is malformed.
    pub(crate) fn delete_role_object_grants_for_role(
        &self,
        role_id: u64,
    ) -> Result<Vec<RoleObjectGrantMeta>, CassieError> {
        if role_id == 0 {
            return Ok(Vec::new());
        }
        let mut tx = self.begin_schema_rw_tx()?;
        let removed = Self::delete_role_object_grants_for_role_to_tx(&mut tx, role_id)?;
        if removed.is_empty() {
            return Ok(removed);
        }
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)?;
        Ok(removed)
    }

    pub(crate) fn delete_role_object_grants_for_role_to_tx(
        tx: &mut AdapterTransaction,
        role_id: u64,
    ) -> Result<Vec<RoleObjectGrantMeta>, CassieError> {
        if role_id == 0 {
            return Ok(Vec::new());
        }
        let prefix = key_encoding::role_object_grant_prefix();
        let entries = collect_scan(
            tx.scan(&Query::new().prefix(prefix.into()))
                .map_err(CassieError::from)?,
        )?;
        let mut removed = Vec::new();
        for (key, value) in entries {
            let grant = decode_record(&key, &value)?;
            if grant.role_id == role_id {
                tx.delete(key).map_err(CassieError::from)?;
                removed.push(grant);
            }
        }
        removed.sort_by_key(RoleObjectGrantMeta::catalog_key);
        Ok(removed)
    }

    pub(crate) fn delete_role_object_grants_for_targets_to_tx(
        tx: &mut AdapterTransaction,
        database_family: &str,
        targets: &[RoleGrantTarget],
    ) -> Result<Vec<RoleObjectGrantMeta>, CassieError> {
        let prefix = key_encoding::role_object_grant_prefix();
        let entries = collect_scan(
            tx.scan(&Query::new().prefix(prefix.into()))
                .map_err(CassieError::from)?,
        )?;
        let mut removed = Vec::new();
        for (key, value) in entries {
            let grant = decode_record(&key, &value)?;
            if grant.database_family == database_family && targets.contains(&grant.target) {
                tx.delete(key).map_err(CassieError::from)?;
                removed.push(grant);
            }
        }
        removed.sort_by_key(RoleObjectGrantMeta::catalog_key);
        Ok(removed)
    }
}

fn decode_record(key: &[u8], raw: &[u8]) -> Result<RoleObjectGrantMeta, CassieError> {
    let grant: RoleObjectGrantMeta = serde_json::from_slice(raw).map_err(|error| {
        CassieError::Parse(format!("invalid role object grant metadata: {error}"))
    })?;
    validate_record(&grant).map_err(|error| match error {
        CassieError::Unsupported(message) => CassieError::Parse(message),
        other => other,
    })?;
    if key != key_encoding::role_object_grant_key(&grant) {
        return Err(CassieError::Parse(
            "role object grant key does not match its identity".to_owned(),
        ));
    }
    Ok(grant)
}

fn validate_identity(grant: &RoleObjectGrantMeta) -> Result<(), CassieError> {
    if grant.database_family.is_empty() || grant.role_id == 0 {
        return Err(CassieError::Unsupported(
            "role object grant requires a database family and nonzero role identity".to_owned(),
        ));
    }
    validate_target_identity(&grant.target)
}

fn validate_target_identity(target: &RoleGrantTarget) -> Result<(), CassieError> {
    let object_id = match target {
        RoleGrantTarget::Schema(object_id) | RoleGrantTarget::Table(object_id) => *object_id,
    };
    if object_id == 0 {
        return Err(CassieError::Unsupported(
            "role object grant requires a nonzero schema or table identity".to_owned(),
        ));
    }
    Ok(())
}

fn validate_privileges(
    target: &RoleGrantTarget,
    privileges: &[RoleObjectPrivilege],
) -> Result<(), CassieError> {
    if privileges.is_empty() {
        return Err(CassieError::Unsupported(
            "role object grant requires at least one direct privilege".to_owned(),
        ));
    }
    let supported = privileges.iter().all(|privilege| match target {
        RoleGrantTarget::Schema(_) => *privilege == RoleObjectPrivilege::Usage,
        RoleGrantTarget::Table(_) => matches!(
            privilege,
            RoleObjectPrivilege::Select
                | RoleObjectPrivilege::Insert
                | RoleObjectPrivilege::Update
                | RoleObjectPrivilege::Delete
        ),
    });
    if !supported {
        return Err(CassieError::Unsupported(
            "role object grant privilege is unsupported for its target".to_owned(),
        ));
    }
    Ok(())
}

fn validate_record(grant: &RoleObjectGrantMeta) -> Result<(), CassieError> {
    validate_identity(grant)?;
    let privileges = grant.privileges.iter().copied().collect::<Vec<_>>();
    validate_privileges(&grant.target, &privileges)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::super::{key_encoding, Midge, StorageFamily};
    use crate::app::CassieError;
    use crate::catalog::{RoleGrantTarget, RoleMeta, RoleObjectGrantMeta, RoleObjectPrivilege};

    fn grant(
        database_family: &str,
        role_id: u64,
        target: RoleGrantTarget,
        privileges: &[RoleObjectPrivilege],
    ) -> RoleObjectGrantMeta {
        RoleObjectGrantMeta {
            database_family: database_family.to_owned(),
            role_id,
            target,
            privileges: privileges.iter().copied().collect::<BTreeSet<_>>(),
        }
    }

    fn midge() -> (Midge, String) {
        let midge = Midge::new_in_memory("postgres").expect("in-memory Midge");
        midge.ensure_families_ready().expect("bootstrap families");
        let family = midge
            .get_database("postgres")
            .expect("read default database")
            .expect("default database")
            .physical_family;
        (midge, family)
    }

    #[test]
    fn should_persist_and_revoke_direct_role_privileges_in_schema_family() {
        // Arrange
        let (midge, family) = midge();
        let schema = grant(&family, 22, RoleGrantTarget::Schema(31), &[]);
        let table = grant(&family, 22, RoleGrantTarget::Table(41), &[]);

        // Act
        let granted_schema = midge
            .update_role_object_grant(&schema, &[RoleObjectPrivilege::Usage], true)
            .expect("grant schema USAGE");
        let granted_table = midge
            .update_role_object_grant(
                &table,
                &[RoleObjectPrivilege::Select, RoleObjectPrivilege::Insert],
                true,
            )
            .expect("grant table privileges");
        let revoked_table = midge
            .update_role_object_grant(&table, &[RoleObjectPrivilege::Select], false)
            .expect("revoke table SELECT");
        let listed = midge.list_role_object_grants().expect("list grants");

        // Assert
        assert_eq!(
            granted_schema.expect("schema grant").privileges,
            [RoleObjectPrivilege::Usage].into_iter().collect()
        );
        assert_eq!(
            granted_table.expect("table grant").privileges,
            [RoleObjectPrivilege::Select, RoleObjectPrivilege::Insert]
                .into_iter()
                .collect()
        );
        assert_eq!(
            revoked_table.expect("remaining table grant").privileges,
            [RoleObjectPrivilege::Insert].into_iter().collect()
        );
        assert_eq!(listed.len(), 2);
        assert!(listed
            .windows(2)
            .all(|pair| pair[0].catalog_key() <= pair[1].catalog_key()));
    }

    #[test]
    fn should_delete_only_matching_database_targets_and_role_records() {
        // Arrange
        let (midge, family) = midge();
        midge
            .create_database("role_grant_other", None)
            .expect("create second database");
        let other_family = midge
            .get_database("role_grant_other")
            .expect("read second database")
            .expect("second database")
            .physical_family;
        let target = RoleGrantTarget::Table(41);
        let records = [
            grant(&family, 22, target.clone(), &[RoleObjectPrivilege::Select]),
            grant(&family, 23, target.clone(), &[RoleObjectPrivilege::Insert]),
            grant(
                &family,
                22,
                RoleGrantTarget::Schema(31),
                &[RoleObjectPrivilege::Usage],
            ),
            grant(
                &other_family,
                22,
                target.clone(),
                &[RoleObjectPrivilege::Select],
            ),
        ];
        for record in &records {
            midge
                .update_role_object_grant(
                    record,
                    &record.privileges.iter().copied().collect::<Vec<_>>(),
                    true,
                )
                .expect("persist grant");
        }

        // Act
        let removed_target = midge
            .delete_role_object_grants_for_targets(&family, std::slice::from_ref(&target))
            .expect("delete matching target grants");
        let after_targets = midge
            .list_role_object_grants()
            .expect("list grants after target deletion");
        let removed_role = midge
            .delete_role_object_grants_for_role(22)
            .expect("delete all records for role");
        let remaining = midge
            .list_role_object_grants()
            .expect("list remaining grants");

        // Assert
        assert_eq!(removed_target.len(), 2);
        assert!(removed_target
            .iter()
            .all(|grant| grant.database_family == family && grant.target == target));
        assert_eq!(after_targets.len(), 2);
        assert!(after_targets
            .iter()
            .any(|grant| { grant.database_family == other_family && grant.target == target }));
        assert!(after_targets
            .iter()
            .any(|grant| grant.target == RoleGrantTarget::Schema(31)));
        assert_eq!(removed_role.len(), 2);
        assert_eq!(remaining, Vec::new());
    }

    #[test]
    fn should_reject_role_grant_record_when_encoded_key_disagrees_with_value() {
        // Arrange
        let (midge, family) = midge();
        let key_record = grant(
            &family,
            22,
            RoleGrantTarget::Schema(31),
            &[RoleObjectPrivilege::Usage],
        );
        let value_record = grant(
            &family,
            22,
            RoleGrantTarget::Schema(32),
            &[RoleObjectPrivilege::Usage],
        );
        let key = key_encoding::role_object_grant_key(&key_record);
        let value = serde_json::to_vec(&value_record).expect("serialize mismatched record");
        midge
            .raw_put(StorageFamily::Schema, &key, &value)
            .expect("seed mismatched key/value record");

        // Act
        let result = midge.list_role_object_grants();

        // Assert
        assert!(matches!(result, Err(CassieError::Parse(_))));
    }

    #[test]
    fn should_reject_role_grants_with_invalid_identity_or_target_privilege() {
        // Arrange
        let (midge, family) = midge();
        let zero_role = grant(
            &family,
            0,
            RoleGrantTarget::Schema(31),
            &[RoleObjectPrivilege::Usage],
        );
        let wrong_schema_privilege = grant(&family, 22, RoleGrantTarget::Schema(31), &[]);

        // Act
        let zero_role_result =
            midge.update_role_object_grant(&zero_role, &[RoleObjectPrivilege::Usage], true);
        let wrong_target_result = midge.update_role_object_grant(
            &wrong_schema_privilege,
            &[RoleObjectPrivilege::Select],
            true,
        );

        // Assert
        assert!(matches!(zero_role_result, Err(CassieError::Unsupported(_))));
        assert!(matches!(
            wrong_target_result,
            Err(CassieError::Unsupported(_))
        ));
    }

    #[test]
    fn should_remove_role_and_grants_in_one_schema_family_lifecycle() {
        // Arrange
        let (midge, family) = midge();
        let mut role = RoleMeta::login("grant_reader", "hash".to_owned());
        role.object_id = 22;
        midge.put_role(&role).expect("persist role");
        let schema = grant(
            &family,
            role.object_id,
            RoleGrantTarget::Schema(31),
            &[RoleObjectPrivilege::Usage],
        );
        midge
            .update_role_object_grant(&schema, &[RoleObjectPrivilege::Usage], true)
            .expect("persist schema grant");

        // Act
        midge.delete_role(&role.name).expect("delete role");

        // Assert
        assert!(midge
            .get_role(&role.name)
            .expect("read deleted role")
            .is_none());
        assert_eq!(
            midge
                .list_role_object_grants()
                .expect("list grants after role drop")
                .len(),
            0
        );
    }
}
