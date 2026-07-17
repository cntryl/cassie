use std::net::SocketAddr;

use super::{CassieRuntimeConfig, CassieRuntimeConfigError};

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

pub(crate) fn validate_pgwire_listener_transport(
    config: &CassieRuntimeConfig,
    listener: SocketAddr,
) -> Result<(), CassieRuntimeConfigError> {
    validate_pair(
        config.pgwire_tls_cert_file.as_ref(),
        config.pgwire_tls_key_file.as_ref(),
        CassieRuntimeConfigError::PgwireTlsPair,
    )?;
    if config.allow_insecure_non_loopback_listen {
        return Ok(());
    }
    require_tls_for_address(
        listener,
        config.pgwire_tls_cert_file.is_some(),
        |listener| CassieRuntimeConfigError::PgwireTlsRequired { listener },
    )
}

pub(crate) fn validate_rest_listener_transport(
    certificate: Option<&str>,
    key: Option<&str>,
    allow_insecure_non_loopback: bool,
    listener: SocketAddr,
) -> Result<(), CassieRuntimeConfigError> {
    validate_pair(certificate, key, CassieRuntimeConfigError::RestTlsPair)?;
    if allow_insecure_non_loopback {
        return Ok(());
    }
    require_tls_for_address(listener, certificate.is_some(), |listener| {
        CassieRuntimeConfigError::RestTlsRequired { listener }
    })
}

fn validate_pair<T: ?Sized>(
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

fn require_tls_for_address(
    listener: SocketAddr,
    tls_configured: bool,
    error: impl FnOnce(String) -> CassieRuntimeConfigError,
) -> Result<(), CassieRuntimeConfigError> {
    if listener.ip().is_loopback() || tls_configured {
        Ok(())
    } else {
        Err(error(listener.to_string()))
    }
}
