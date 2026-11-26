use std::sync::Arc;

use crate::app::Cassie;
use crate::config::CassieRuntimeConfig;
use tokio::sync::{Notify, Semaphore};

/// # Errors
///
/// Returns an error when validation, storage, or execution fails.
pub async fn run(
    addr: String,
    cassie: Arc<Cassie>,
    config: CassieRuntimeConfig,
) -> Result<(), crate::app::CassieError> {
    run_with_shutdown(addr, cassie, config, Arc::new(Notify::new())).await
}

/// # Errors
///
/// Returns an error when validation, storage, or execution fails.
pub async fn run_with_shutdown(
    addr: String,
    cassie: Arc<Cassie>,
    config: CassieRuntimeConfig,
    shutdown: Arc<Notify>,
) -> Result<(), crate::app::CassieError> {
    if let Ok(requested_address) = addr.parse::<std::net::SocketAddr>() {
        cassie.validate_network_listener(requested_address)?;
        crate::config::validate_pgwire_listener_transport(&config, requested_address)?;
    }
    let tls_config = crate::pgwire::tls::load_server_config(
        config.pgwire_tls_cert_file.as_deref(),
        config.pgwire_tls_key_file.as_deref(),
    )?;
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| crate::app::CassieError::Execution(e.to_string()))?;
    serve(listener, cassie, config, tls_config, shutdown).await
}

/// Serve an already bound listener, retaining ownership of its assigned port.
///
/// # Errors
///
/// Returns an error when listener policy or TLS identity validation fails.
pub async fn run_with_listener(
    listener: tokio::net::TcpListener,
    cassie: Arc<Cassie>,
    config: CassieRuntimeConfig,
) -> Result<(), crate::app::CassieError> {
    run_with_listener_and_shutdown(listener, cassie, config, Arc::new(Notify::new())).await
}

/// Serve an already bound listener until shutdown is requested.
///
/// The actual bound address receives the same authentication and transport
/// validation as an address bound by [`run_with_shutdown`].
///
/// # Errors
///
/// Returns an error when listener policy or TLS identity validation fails.
pub async fn run_with_listener_and_shutdown(
    listener: tokio::net::TcpListener,
    cassie: Arc<Cassie>,
    config: CassieRuntimeConfig,
    shutdown: Arc<Notify>,
) -> Result<(), crate::app::CassieError> {
    let address = listener
        .local_addr()
        .map_err(|error| crate::app::CassieError::Execution(error.to_string()))?;
    cassie.validate_network_listener(address)?;
    crate::config::validate_pgwire_listener_transport(&config, address)?;
    let tls_config = crate::pgwire::tls::load_server_config(
        config.pgwire_tls_cert_file.as_deref(),
        config.pgwire_tls_key_file.as_deref(),
    )?;
    serve(listener, cassie, config, tls_config, shutdown).await
}

async fn serve(
    listener: tokio::net::TcpListener,
    cassie: Arc<Cassie>,
    config: CassieRuntimeConfig,
    tls_config: Option<Arc<rustls::ServerConfig>>,
    shutdown: Arc<Notify>,
) -> Result<(), crate::app::CassieError> {
    let listen_address = listener
        .local_addr()
        .map_err(|error| crate::app::CassieError::Execution(error.to_string()))?;
    cassie.validate_network_listener(listen_address)?;
    crate::config::validate_pgwire_listener_transport(&config, listen_address)?;
    let require_tls =
        !config.allow_insecure_non_loopback_listen && !listen_address.ip().is_loopback();
    let trust_path = config.pgwire_aws_iam_trust_file.as_deref();
    cassie.validate_aws_iam_pgwire_selector(trust_path.is_some())?;
    if trust_path.is_some() && (tls_config.is_none() || !cassie.authentication_enabled()) {
        return Err(crate::app::CassieError::Execution(
            "AWS IAM pgwire authentication requires TLS and a root password".into(),
        ));
    }
    let iam = trust_path
        .map(std::path::Path::new)
        .map(super::iam::Verifier::from_path)
        .transpose()
        .map_err(|_| {
            crate::app::CassieError::Execution("invalid AWS IAM pgwire trust file".into())
        })?
        .map(Arc::new);
    tracing::info!(target: "pgwire", address = %listen_address, "listening");
    let admission = Arc::new(Semaphore::new(config.limits.pgwire_max_connections.max(1)));

    loop {
        tokio::select! {
            biased;
            () = shutdown.notified() => {
                tracing::info!(target: "pgwire", address = %listen_address, "shutdown requested");
                break;
            }
            accept = listener.accept() => {
                match accept {
                    Ok((socket, peer)) => {
                        let peer_addr = format!("{peer}");
                        tracing::info!(target: "pgwire", peer = %peer_addr, "accepted");
                        let Ok(permit) = admission.clone().try_acquire_owned() else {
                            tracing::warn!(target: "pgwire", peer = %peer_addr, "connection admission rejected");
                            if require_tls {
                                drop(socket);
                                continue;
                            }
                            tokio::spawn(async move {
                                crate::pgwire::connection::reject_too_many_connections(socket).await;
                            });
                            continue;
                        };
                        let cassie = cassie.clone();
                        let config = config.clone();
                        let tls_config = tls_config.clone();
                        let iam = iam.clone();
                        tokio::spawn(async move {
                            let () = crate::pgwire::connection::worker_lease::with_connection_lease(
                                permit,
                                Box::pin(crate::pgwire::connection::run_connection_with_iam(
                                    socket,
                                    cassie,
                                    config,
                                    tls_config,
                                    require_tls,
                                    peer.ip(),
                                    iam,
                                )),
                            ).await;
                        });
                    }
                    Err(e) => {
                        tracing::warn!(target: "pgwire", error = %e, "accept failed");
                    }
                }
            }
        }
    }

    Ok(())
}
