use std::net::SocketAddr;

use super::{CassieRuntimeConfig, CassieRuntimeConfigError};

pub(super) fn validate_bootstrap_password(
    config: &CassieRuntimeConfig,
) -> Result<(), CassieRuntimeConfigError> {
    if config.password != "postgres" {
        return Ok(());
    }
    for listener in [&config.pgwire_listen, &config.rest_listen] {
        let Ok(address) = listener.parse::<SocketAddr>() else {
            continue;
        };
        if !address.ip().is_loopback() {
            return Err(CassieRuntimeConfigError::UnsafeDefaultPassword {
                listener: listener.clone(),
            });
        }
    }
    Ok(())
}

pub(super) fn validate_transport_tls_policy(
    config: &CassieRuntimeConfig,
) -> Result<(), CassieRuntimeConfigError> {
    validate_pair(
        config.pgwire_tls_cert_file.as_ref(),
        config.pgwire_tls_key_file.as_ref(),
        CassieRuntimeConfigError::PgwireTlsPair,
    )?;
    validate_pair(
        config.rest_tls_cert_file.as_ref(),
        config.rest_tls_key_file.as_ref(),
        CassieRuntimeConfigError::RestTlsPair,
    )?;
    if !config.allow_insecure_non_loopback_listen {
        require_tls_for_non_loopback(
            &config.pgwire_listen,
            config.pgwire_tls_cert_file.is_some(),
            |listener| CassieRuntimeConfigError::PgwireTlsRequired { listener },
        )?;
        require_tls_for_non_loopback(
            &config.rest_listen,
            config.rest_tls_cert_file.is_some(),
            |listener| CassieRuntimeConfigError::RestTlsRequired { listener },
        )?;
    }
    Ok(())
}

fn validate_pair<T>(
    certificate: Option<&T>,
    key: Option<&T>,
    error: CassieRuntimeConfigError,
) -> Result<(), CassieRuntimeConfigError> {
    if certificate.is_some() == key.is_some() {
        Ok(())
    } else {
        Err(error)
    }
}

fn require_tls_for_non_loopback(
    listener: &str,
    tls_configured: bool,
    error: impl FnOnce(String) -> CassieRuntimeConfigError,
) -> Result<(), CassieRuntimeConfigError> {
    let Ok(address) = listener.parse::<SocketAddr>() else {
        return Ok(());
    };
    if address.ip().is_loopback() || tls_configured {
        Ok(())
    } else {
        Err(error(listener.to_string()))
    }
}
