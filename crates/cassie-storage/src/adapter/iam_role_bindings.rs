//! Durable exact IAM role bindings in the shared role catalog family.

use serde::{Deserialize, Serialize};

use super::{AdapterTransaction, CassieError, Midge, Query, RoleMeta};

const KEY_FAMILY: &[u8] = b"pgwire-aws-iam-role";

#[derive(Deserialize, Serialize)]
struct RoleBindingRecord {
    role_name: String,
    role_id: u64,
    binding_id: u64,
}

fn key(issuer: &str, arn: &str) -> Vec<u8> {
    crate::key_codec::encode_key(KEY_FAMILY, &[issuer.as_bytes(), arn.as_bytes()])
}

fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, CassieError> {
    serde_json::from_slice(bytes)
        .map_err(|_| CassieError::Parse("invalid AWS IAM binding or role metadata".into()))
}

impl Midge {
    /// Looks up a published role and binding together in one committed snapshot.
    ///
    /// # Errors
    /// Returns a storage or metadata error if the binding cannot be read.
    pub fn aws_iam_role(
        &self,
        issuer: &str,
        arn: &str,
    ) -> Result<Option<(RoleMeta, u64)>, CassieError> {
        let tx = self.begin_schema_readonly_tx()?;
        let Some(raw) = tx.get(&key(issuer, arn)).map_err(CassieError::from)? else {
            return Ok(None);
        };
        let binding: RoleBindingRecord = decode(&raw)?;
        let Some(raw) = tx
            .get(&Self::role_key(&binding.role_name))
            .map_err(CassieError::from)?
        else {
            return Ok(None);
        };
        let role: RoleMeta = decode(&raw)?;
        Ok((role.name == binding.role_name
            && role.object_id == binding.role_id
            && binding.binding_id != 0
            && external_login(&role)
            && role.can_login)
            .then_some((role, binding.binding_id)))
    }

    /// Publishes or removes a binding for the specified non-admin role identity.
    ///
    /// # Errors
    /// Returns an error if the role identity is obsolete or the store cannot update the binding.
    pub fn update_aws_iam_role_binding(
        &self,
        expected_role: &RoleMeta,
        issuer: &str,
        arn: &str,
        bind: bool,
    ) -> Result<(), CassieError> {
        self.with_schema_publication(|| {
            let mut tx = self.begin_schema_rw_tx()?;
            let role: RoleMeta = decode(
                &tx.get(&Self::role_key(&expected_role.name))
                    .map_err(CassieError::from)?
                    .ok_or(CassieError::InvalidQuery(
                        "role is absent or not eligible for AWS IAM binding".into(),
                    ))?,
            )?;
            if role.name != expected_role.name
                || role.object_id != expected_role.object_id
                || !external_login(&role)
            {
                return Err(CassieError::InvalidQuery(
                    "role is absent or not eligible for AWS IAM binding".into(),
                ));
            }
            let key = key(issuer, arn);
            let existing = tx
                .get(&key)
                .map_err(CassieError::from)?
                .map(|raw| decode::<RoleBindingRecord>(&raw))
                .transpose()?;
            if let Some(current) = existing.as_ref() {
                if current.role_name != role.name || current.role_id != role.object_id {
                    return Err(CassieError::InvalidQuery(
                        "AWS IAM identity belongs to another Cassie role".into(),
                    ));
                }
            }
            match (bind, existing) {
                (true, None) => {
                    let binding = RoleBindingRecord {
                        role_name: role.name,
                        role_id: role.object_id,
                        binding_id: Self::allocate_object_id_to_tx(&mut tx)?,
                    };
                    let bytes = serde_json::to_vec(&binding).map_err(|_| {
                        CassieError::Execution("cannot encode AWS IAM binding".into())
                    })?;
                    tx.put(key, bytes, None).map_err(CassieError::from)?;
                }
                (false, Some(_)) => tx.delete(key).map_err(CassieError::from)?,
                _ => return Ok(()),
            }
            tx.commit(self.write_options_sync())
                .map_err(CassieError::from)
        })
    }

    pub(super) fn delete_aws_iam_bindings_to_tx(
        tx: &mut AdapterTransaction,
        role_id: u64,
    ) -> Result<(), CassieError> {
        let keys = tx
            .scan(&Query::new().prefix(crate::key_codec::encode_prefix(KEY_FAMILY, &[]).into()))
            .map_err(CassieError::from)?
            .filter_map(|entry| match entry.map_err(CassieError::from) {
                Ok((key, value)) => match decode::<RoleBindingRecord>(&value) {
                    Ok(binding) if binding.role_id == role_id => Some(Ok(key)),
                    Ok(_) => None,
                    Err(error) => Some(Err(error)),
                },
                Err(error) => Some(Err(error)),
            })
            .collect::<Result<Vec<_>, _>>()?;
        for key in keys {
            tx.delete(key.to_vec()).map_err(CassieError::from)?;
        }
        Ok(())
    }
}

fn external_login(role: &RoleMeta) -> bool {
    role.aws_iam_login && !role.is_admin && role.object_id != 0 && role.password_hash.is_none()
}
