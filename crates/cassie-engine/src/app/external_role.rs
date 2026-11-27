//! A verified external login can supply database scope for one durable Cassie role.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use super::{Cassie, CassieError, CassieSession, NetworkPrincipal, NetworkRole, RoleMeta};

/// Authority recorded after the transport validates a browser login.
#[doc(hidden)]
#[derive(Debug)]
pub struct ExternalRoleAuthority {
    role: String,
    role_id: u64,
    databases: BTreeSet<String>,
    all_databases: bool,
    expires_at: u64,
}

impl ExternalRoleAuthority {
    pub(super) fn allows_database(&self, database: &str) -> bool {
        self.all_databases || self.databases.contains(database)
    }

    pub(super) fn matches_role(&self, role: &RoleMeta) -> bool {
        role.object_id == self.role_id
            && role.name == self.role
            && !role.is_admin
            && SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .is_ok_and(|now| now.as_secs() < self.expires_at)
    }
}

impl CassieSession {
    pub(crate) fn external_database_access(&self, database: &str) -> Option<bool> {
        self.external_role
            .as_ref()
            .map(|scope| scope.allows_database(database))
    }
}

impl Cassie {
    /// Maps verified token permissions to the current durable non-admin role.
    ///
    /// # Errors
    /// Rejects unknown or admin roles, expired authority and missing database grants.
    #[doc(hidden)]
    pub fn capture_external_role(
        &self,
        role: &str,
        permissions: &[String],
        expires_at: u64,
    ) -> Result<Arc<ExternalRoleAuthority>, CassieError> {
        let stored = self.lookup_role(role)?.ok_or(CassieError::Unauthorized)?;
        if stored.is_admin || stored.object_id == 0 || stored.name == self.auth_user {
            return Err(CassieError::Unauthorized);
        }
        let databases = permissions
            .iter()
            .filter_map(|permission| permission.strip_prefix("db:"))
            .filter(|name| {
                !name.is_empty()
                    && !name.contains('*')
                    && !name.chars().any(|character| character.is_control())
                    && name.trim() == *name
            })
            .map(str::to_owned)
            .collect::<BTreeSet<_>>();
        let all_databases = permissions.iter().any(|permission| permission == "db:*");
        if databases.is_empty() && !all_databases {
            return Err(CassieError::Unauthorized);
        }
        let authority = ExternalRoleAuthority {
            role: stored.name.clone(),
            role_id: stored.object_id,
            databases,
            all_databases,
            expires_at,
        };
        if !authority.matches_role(&stored) {
            return Err(CassieError::Unauthorized);
        }
        Ok(Arc::new(authority))
    }

    /// Revalidates stored authority before producing a network principal.
    ///
    /// # Errors
    /// Rejects an expired session or a stale role identity.
    #[doc(hidden)]
    pub fn external_principal(
        &self,
        authority: &Arc<ExternalRoleAuthority>,
    ) -> Result<NetworkPrincipal, CassieError> {
        let role = self.check_external_role(authority, &authority.role)?;
        let mut session = CassieSession::authenticated(role.name.clone(), None, false);
        session.external_role = Some(Arc::clone(authority));
        Ok(NetworkPrincipal {
            session,
            role: NetworkRole::external(role.name, Arc::clone(authority)),
        })
    }

    pub(super) fn check_external_role(
        &self,
        authority: &ExternalRoleAuthority,
        user: &str,
    ) -> Result<RoleMeta, CassieError> {
        self.lookup_role(user)?
            .filter(|role| authority.matches_role(role))
            .ok_or(CassieError::Unauthorized)
    }
}
