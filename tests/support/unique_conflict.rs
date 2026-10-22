//! Instrumented pgwire sessions for unresolved unique-key ownership tests.

use cassie::app::Cassie;
use cassie::config::CassieRuntimeConfig;
use std::sync::Arc;
use std::time::Duration;
use tokio_postgres::NoTls;

pub struct Fixture {
    pub cassie: Cassie,
    pub owner: tokio_postgres::Client,
    pub contender: tokio_postgres::Client,
    pub owner_connection: tokio::task::JoinHandle<()>,
    pub contender_connection: tokio::task::JoinHandle<()>,
    pub server: tokio::task::JoinHandle<()>,
}

pub struct OwnerWait<T> {
    pub request_entered_engine: bool,
    pub early_completion: Option<Result<T, tokio::task::JoinError>>,
}

pub async fn observe_owner_wait<T>(
    cassie: &Cassie,
    contender_query: &mut tokio::task::JoinHandle<T>,
) -> OwnerWait<T> {
    let request_entered_engine = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if contender_query.is_finished() {
                return false;
            }
            if cassie.metrics()["runtime"]["running_queries"]
                .as_u64()
                .is_some_and(|running| running > 0)
            {
                return true;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap_or(false);
    let early_completion = tokio::time::timeout(Duration::from_millis(500), contender_query)
        .await
        .ok();
    OwnerWait {
        request_entered_engine,
        early_completion,
    }
}

pub async fn start() -> Fixture {
    let mut server_config = CassieRuntimeConfig::from_env().expect("runtime config");
    server_config.password = "postgres".to_string();
    let cassie = Cassie::new_in_memory_with_config(server_config.clone())
        .expect("create in-memory instrumented Cassie instance");
    cassie
        .startup()
        .expect("start instrumented Cassie instance");
    let server_app = Arc::new(cassie.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind pgwire listener");
    let addr = listener.local_addr().expect("listener address");
    let server = tokio::spawn(async move {
        let _ =
            cassie::pgwire::server::run_with_listener(listener, server_app, server_config).await;
    });

    let mut connection_config = tokio_postgres::Config::new();
    connection_config
        .host("127.0.0.1")
        .port(addr.port())
        .user("root")
        .password("postgres")
        .dbname("postgres");
    let (owner, owner_connection) = connection_config
        .connect(NoTls)
        .await
        .expect("connect unique-key owner");
    let owner_connection = tokio::spawn(async move {
        owner_connection
            .await
            .expect("owner pgwire connection should stay healthy");
    });
    let (contender, contender_connection) = connection_config
        .connect(NoTls)
        .await
        .expect("connect competing session");
    let contender_connection = tokio::spawn(async move {
        contender_connection
            .await
            .expect("competing pgwire connection should stay healthy");
    });
    Fixture {
        cassie,
        owner,
        contender,
        owner_connection,
        contender_connection,
        server,
    }
}
