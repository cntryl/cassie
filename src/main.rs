use cassie::{Cassie, CassieError, CassieRuntimeConfig};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), CassieError> {
    tracing_subscriber::fmt().with_env_filter("info").init();

    let config = CassieRuntimeConfig::from_env();
    let cassie = Arc::new(Cassie::new()?);
    cassie.startup().await?;

    let pgwire = tokio::spawn(cassie::pgwire::server::run(
        config.pgwire_listen.clone(),
        cassie.clone(),
        config.clone(),
    ));
    let rest = tokio::spawn(cassie::rest::router::run(
        config.rest_listen.clone(),
        cassie.as_ref().clone(),
    ));

    let (pgwire_result, rest_result) = tokio::join!(pgwire, rest);
    let pgwire_result = pgwire_result.map_err(|error| CassieError::Execution(error.to_string()))?;
    let rest_result = rest_result.map_err(|error| CassieError::Execution(error.to_string()))?;

    pgwire_result?;
    rest_result?;

    Ok(())
}
