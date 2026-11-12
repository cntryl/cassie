use crate::catalog::{RoleGrantTarget, RoleObjectGrantMeta};

use super::{key, prefix, FAMILY_ROLE_OBJECT_GRANT};

pub(super) fn role_object_grant_key(grant: &RoleObjectGrantMeta) -> Vec<u8> {
    let role_id = grant.role_id.to_be_bytes();
    let (target_kind, object_id) = match &grant.target {
        RoleGrantTarget::Schema(object_id) => (b"schema".as_slice(), *object_id),
        RoleGrantTarget::Table(object_id) => (b"table".as_slice(), *object_id),
    };
    let object_id = object_id.to_be_bytes();
    key(
        FAMILY_ROLE_OBJECT_GRANT,
        &[
            grant.database_family.as_bytes(),
            role_id.as_slice(),
            target_kind,
            object_id.as_slice(),
        ],
    )
}

pub(super) fn role_object_grant_prefix() -> Vec<u8> {
    prefix(FAMILY_ROLE_OBJECT_GRANT, &[])
}
