use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RoleMeta {
    pub name: String,
    pub can_login: bool,
    pub is_admin: bool,
    pub password_hash: Option<String>,
    #[serde(default)]
    pub database_grants: Option<BTreeSet<String>>,
}

impl RoleMeta {
    pub fn new(
        name: impl AsRef<str>,
        can_login: bool,
        is_admin: bool,
        password_hash: Option<String>,
    ) -> Self {
        Self {
            name: normalize_role_name(name),
            can_login,
            is_admin,
            password_hash,
            database_grants: Some(BTreeSet::new()),
        }
    }

    pub fn bootstrap_admin(name: impl AsRef<str>, password_hash: Option<String>) -> Self {
        Self::new(name, true, true, password_hash)
    }

    pub fn login(name: impl AsRef<str>, password_hash: String) -> Self {
        Self::new(name, true, false, Some(password_hash))
    }

    #[must_use]
    pub fn can_access_database(&self, database: &str) -> bool {
        self.is_admin
            || self.database_grants.as_ref().is_some_and(|grants| {
                grants
                    .iter()
                    .any(|grant| grant.eq_ignore_ascii_case(database))
            })
    }

    pub fn grant_database(&mut self, database: impl AsRef<str>) {
        self.database_grants
            .get_or_insert_with(BTreeSet::new)
            .insert(database.as_ref().trim().to_ascii_lowercase());
    }

    pub fn revoke_database(&mut self, database: &str) {
        if let Some(grants) = &mut self.database_grants {
            grants.retain(|grant| !grant.eq_ignore_ascii_case(database));
        }
    }
}

pub fn normalize_role_name(name: impl AsRef<str>) -> String {
    name.as_ref().trim().to_ascii_lowercase()
}
