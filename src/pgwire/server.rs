use std::sync::Arc;

use crate::app::Cassie;
use crate::config::CassieRuntimeConfig;

pub async fn run(
    addr: String,
    cassie: Arc<Cassie>,
    config: CassieRuntimeConfig,
) -> Result<(), crate::app::CassieError> {
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| crate::app::CassieError::Execution(e.to_string()))?;
    tracing::info!(target: "pgwire", address = %addr, "listening");

    loop {
        let accept = listener.accept().await;
        match accept {
            Ok((socket, peer)) => {
                let peer_addr = format!("{}", peer);
                tracing::info!(target: "pgwire", peer = %peer_addr, "accepted");
                let cassie = cassie.clone();
                let config = config.clone();
                tokio::spawn(async move {
                    let _ = crate::pgwire::connection::run_connection(socket, cassie, config).await;
                });
            }
            Err(e) => {
                tracing::warn!(target: "pgwire", error = %e, "accept failed");
            }
        }
    }

    #[allow(unreachable_code)]
    Ok(())
}
