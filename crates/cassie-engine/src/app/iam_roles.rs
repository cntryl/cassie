//! IAM authorities bind to durable role and binding identities.

use std::net::IpAddr;
use std::sync::Arc;

use super::auth_rate_limit::AuthAttempt;
use super::{Cassie, CassieError, CassieSession, CatalogObjectKind, RoleMeta};
use crate::config::AWS_IAM_PGWIRE_SELECTOR;
use crate::sql::ast::AlterAwsIamBinding;

#[cfg(test)]
mod tests;

#[doc(hidden)]
pub struct AwsIamLoginAttempt(AuthAttempt);

#[derive(Debug)]
pub(super) struct AwsIamAuthority {
    issuer: String,
    role_arn: String,
    role_id: u64,
    binding_id: u64,
}

impl CassieSession {
    #[doc(hidden)]
    #[must_use]
    pub fn uses_aws_iam(&self) -> bool {
        self.aws_iam_authority.is_some()
    }

    pub(crate) fn catalog_role(&self, catalog: &crate::catalog::Catalog) -> Option<RoleMeta> {
        catalog.get_role(&self.user).filter(|role| {
            self.aws_iam_authority.as_deref().is_none_or(|authority| {
                role.object_id == authority.role_id && role.aws_iam_login && !role.is_admin
            })
        })
    }
}

impl Cassie {
    /// Checks that constructor and listener settings agree and that the selector is available.
    ///
    /// # Errors
    /// Returns an error if configured authentication cannot use its reserved startup user.
    #[doc(hidden)]
    pub fn validate_aws_iam_pgwire_selector(
        &self,
        listener_enabled: bool,
    ) -> Result<(), CassieError> {
        if listener_enabled != self.aws_iam_login_enabled
            || (listener_enabled && self.lookup_role(AWS_IAM_PGWIRE_SELECTOR)?.is_some())
        {
            return Err(CassieError::Execution(
                "AWS IAM configuration or reserved pgwire startup selector conflicts".into(),
            ));
        }
        Ok(())
    }

    /// Acquires existing per-user and per-peer login admission before verifying a token.
    ///
    /// # Errors
    /// Returns an error when IAM is disabled or admission is exhausted.
    #[doc(hidden)]
    pub fn begin_aws_iam_login(&self, peer: IpAddr) -> Result<AwsIamLoginAttempt, CassieError> {
        if !self.aws_iam_login_enabled {
            return Err(CassieError::Unauthorized);
        }
        self.auth_rate_limiter
            .consume(AWS_IAM_PGWIRE_SELECTOR, peer)
            .map(AwsIamLoginAttempt)
    }

    /// Creates the session for a verified issuer and exact IAM role.
    ///
    /// The network transport verifies the token before calling this boundary.
    ///
    /// # Errors
    /// Returns an error when the role or its database authorization is absent.
    #[doc(hidden)]
    pub fn complete_aws_iam_login(
        &self,
        attempt: AwsIamLoginAttempt,
        issuer: &str,
        role_arn: &str,
        database: Option<String>,
    ) -> Result<CassieSession, CassieError> {
        let (role, binding_id) = self
            .midge
            .aws_iam_role(issuer, role_arn)?
            .ok_or(CassieError::Unauthorized)?;
        let database = database.unwrap_or_else(|| self.default_database.clone());
        self.ensure_database_exists(&database)?;
        if !role.can_access_database(&database) {
            return Err(CassieError::InsufficientPrivilege);
        }
        let mut session = CassieSession::authenticated(role.name, Some(database), false);
        session.aws_iam_authority = Some(Arc::new(AwsIamAuthority {
            issuer: issuer.into(),
            role_arn: role_arn.into(),
            role_id: role.object_id,
            binding_id,
        }));
        self.auth_rate_limiter.refund(&attempt.0);
        Ok(session)
    }

    pub(super) fn check_aws_iam_authority(
        &self,
        authority: &AwsIamAuthority,
        user: &str,
    ) -> Result<RoleMeta, CassieError> {
        self.midge
            .aws_iam_role(&authority.issuer, &authority.role_arn)?
            .filter(|(role, binding_id)| {
                role.name == user
                    && role.object_id == authority.role_id
                    && *binding_id == authority.binding_id
            })
            .map(|(role, _)| role)
            .ok_or(CassieError::Unauthorized)
    }

    pub(crate) fn create_aws_iam_role_in_database(
        &self,
        name: &str,
        if_not_exists: bool,
        database: &str,
    ) -> Result<(), CassieError> {
        let Some(normalized) = self.admit_new_role(name, if_not_exists)? else {
            return Ok(());
        };
        let mut role = RoleMeta::new(normalized, true, false, None);
        role.aws_iam_login = true;
        self.store_new_role(role, database)
    }

    pub(crate) fn alter_aws_iam_role_binding(
        &self,
        name: &str,
        binding: &AlterAwsIamBinding,
    ) -> Result<(), CassieError> {
        let role = self
            .lookup_role(name)?
            .ok_or_else(|| CassieError::CatalogObjectNotFound {
                kind: CatalogObjectKind::Role,
                name: name.to_owned(),
            })?;
        if !role.aws_iam_login || role.is_admin || role.object_id == 0 {
            return Err(CassieError::InsufficientPrivilege);
        }
        if !valid_issuer(&binding.issuer) || !valid_role_arn(&binding.role_arn) {
            return Err(CassieError::Planner(
                "expected an AWS account issuer and IAM role ARN".into(),
            ));
        }
        self.midge.update_aws_iam_role_binding(
            &role,
            &binding.issuer,
            &binding.role_arn,
            binding.bind,
        )?;
        Ok(())
    }
}

fn valid_issuer(issuer: &str) -> bool {
    issuer
        .strip_prefix("https://")
        .and_then(|host| host.strip_suffix(".tokens.sts.global.api.aws"))
        .is_some_and(|label| {
            (1..=63).contains(&label.len())
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
}

fn valid_role_arn(arn: &str) -> bool {
    arn.strip_prefix("arn:aws:iam::")
        .and_then(|rest| rest.split_once(":role/"))
        .is_some_and(|(account, role)| {
            account.len() == 12
                && account.bytes().all(|b| b.is_ascii_digit())
                && (1..=580).contains(&role.len())
                && !role.ends_with('/')
                && role.bytes().all(|b| b.is_ascii_graphic())
        })
}
