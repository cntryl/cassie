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
    let pgwire_address = config.pgwire_listen.parse::<SocketAddr>().map_err(|_| {
        CassieRuntimeConfigError::InvalidPgwireListener {
            listener: config.pgwire_listen.clone(),
        }
    })?;
    let rest_address = config.rest_listen.parse::<SocketAddr>().map_err(|_| {
        CassieRuntimeConfigError::InvalidRestListener {
            listener: config.rest_listen.clone(),
        }
    })?;
    if !config.allow_insecure_non_loopback_listen {
        require_tls_for_address(
            pgwire_address,
            config.pgwire_tls_cert_file.is_some(),
            |listener| CassieRuntimeConfigError::PgwireTlsRequired { listener },
        )?;
        require_tls_for_address(
            rest_address,
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

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    use super::{validate_pgwire_listener_transport, validate_rest_listener_transport};
    use crate::config::CassieRuntimeConfig;

    #[test]
    fn should_allow_loopback_pgwire_without_tls() {
        // Arrange
        let config = CassieRuntimeConfig::default();
        let listener = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 5432);

        // Act
        let result = validate_pgwire_listener_transport(&config, listener);

        // Assert
        assert!(result.is_ok());
    }

    #[test]
    fn should_allow_loopback_rest_without_tls() {
        // Arrange
        let listener = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8080);

        // Act
        let result = validate_rest_listener_transport(None, None, false, listener);

        // Assert
        assert!(result.is_ok());
    }
}
