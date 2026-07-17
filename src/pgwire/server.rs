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
    let listen_address = listener
        .local_addr()
        .map_err(|error| crate::app::CassieError::Execution(error.to_string()))?;
    cassie.validate_network_listener(listen_address)?;
    crate::config::validate_pgwire_listener_transport(&config, listen_address)?;
    let require_tls =
        !config.allow_insecure_non_loopback_listen && !listen_address.ip().is_loopback();
    tracing::info!(target: "pgwire", address = %addr, "listening");
    let admission = Arc::new(Semaphore::new(config.limits.pgwire_max_connections.max(1)));

    loop {
        tokio::select! {
            biased;
            () = shutdown.notified() => {
                tracing::info!(target: "pgwire", address = %addr, "shutdown requested");
                break;
            }
            accept = listener.accept() => {
                match accept {
                    Ok((socket, peer)) => {
                        let peer_addr = format!("{peer}");
                        tracing::info!(target: "pgwire", peer = %peer_addr, "accepted");
                        let Ok(permit) = admission.clone().try_acquire_owned() else {
                            tracing::warn!(target: "pgwire", peer = %peer_addr, "connection admission rejected");
                            tokio::spawn(async move {
                                crate::pgwire::connection::reject_too_many_connections(socket).await;
                            });
                            continue;
                        };
                        let cassie = cassie.clone();
                        let config = config.clone();
                        let tls_config = tls_config.clone();
                        tokio::spawn(async move {
                            let _permit = permit;
                            let () = crate::pgwire::connection::run_connection(
                                socket,
                                cassie,
                                config,
                                tls_config,
                                require_tls,
                            )
                            .await;
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
