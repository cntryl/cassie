use super::*;

use crate::support_pgwire as wire;

#[path = "pgvector/catalogs.rs"]
mod catalogs;
#[path = "pgvector/parameters.rs"]
mod parameters;
#[path = "pgvector/search.rs"]
mod search;
#[path = "pgvector/types.rs"]
mod types;

async fn start_vector_server(
    label: &str,
) -> (
    CompatibilityServer,
    tokio_postgres::Client,
    tokio::task::JoinHandle<()>,
    i32,
) {
    let server = CompatibilityServer::start(label).await;
    let (client, connection) = server.connect().await;
    client
        .batch_execute("CREATE EXTENSION vector")
        .await
        .expect("install the selected pgvector extension profile");
    let oid = client
        .query_one(
            "SELECT type.oid::INT FROM pg_catalog.pg_type AS type \
             JOIN pg_catalog.pg_extension AS extension \
               ON extension.extnamespace = type.typnamespace \
             WHERE extension.extname = 'vector' AND type.typname = 'vector'",
            &[],
        )
        .await
        .expect("read vector's OID from the installed extension catalog")
        .get::<_, i32>(0);
    (server, client, connection, oid)
}
