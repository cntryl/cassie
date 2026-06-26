use cassie::{Cassie, CassieError, CassieRuntimeConfig};
use std::sync::Arc;
use tokio::sync::Notify;

#[tokio::main]
async fn main() -> Result<(), CassieError> {
    tracing_subscriber::fmt().with_env_filter("info").init();

    let config = CassieRuntimeConfig::from_env()?;
    let cassie = Arc::new(Cassie::new()?);
    cassie.startup()?;

    let shutdown = Arc::new(Notify::new());
    let mut pgwire = tokio::spawn(cassie::pgwire::server::run_with_shutdown(
        config.pgwire_listen.clone(),
        cassie.clone(),
        config.clone(),
        shutdown.clone(),
    ));
    let mut rest = tokio::spawn(cassie::rest::router::run_with_shutdown(
        config.rest_listen.clone(),
        cassie.as_ref().clone(),
        shutdown.clone(),
    ));

    let result = async {
        tokio::select! {
            pgwire_result = &mut pgwire => {
                shutdown.notify_waiters();
                let pgwire_result = pgwire_result.map_err(|error| CassieError::Execution(error.to_string()))?;
                let rest_result = join_server(rest).await;
                pgwire_result?;
                rest_result?;
                Ok(())
            }
            rest_result = &mut rest => {
                shutdown.notify_waiters();
                let rest_result = rest_result.map_err(|error| CassieError::Execution(error.to_string()))?;
                let pgwire_result = join_server(pgwire).await;
                rest_result?;
                pgwire_result?;
                Ok(())
            }
            signal = shutdown_signal() => {
                signal?;
                shutdown.notify_waiters();
                join_server(pgwire).await?;
                join_server(rest).await?;
                Ok(())
            }
        }
    }
    .await;

    cassie.shutdown();
    result
}

async fn join_server(
    handle: tokio::task::JoinHandle<Result<(), CassieError>>,
) -> Result<(), CassieError> {
    handle
        .await
        .map_err(|error| CassieError::Execution(error.to_string()))?
}

async fn shutdown_signal() -> Result<(), CassieError> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};

        let mut terminate = signal(SignalKind::terminate())
            .map_err(|error| CassieError::Execution(error.to_string()))?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => {
                result.map_err(|error| CassieError::Execution(error.to_string()))?;
            }
            _ = terminate.recv() => {}
        }

        Ok(())
    }

    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c()
            .await
            .map_err(|error| CassieError::Execution(error.to_string()))?;
        Ok(())
    }
}
