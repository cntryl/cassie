use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RoleMeta {
    pub name: String,
    pub can_login: bool,
    pub is_admin: bool,
    pub password_hash: Option<String>,
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
        }
    }

    pub fn bootstrap_admin(name: impl AsRef<str>, password_hash: Option<String>) -> Self {
        Self::new(name, true, true, password_hash)
    }

    pub fn login(name: impl AsRef<str>, password_hash: String) -> Self {
        Self::new(name, true, false, Some(password_hash))
    }
}

pub fn normalize_role_name(name: impl AsRef<str>) -> String {
    name.as_ref().trim().to_ascii_lowercase()
}
