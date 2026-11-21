//! The clone workflow of #1354 through real PostgreSQL drivers: `tokio-postgres` for the
//! native composite and array path, and `sqlx` for the scalar and JSON path.
//!
//! One instance holds the source database (`postgres`) with the `rooms` graph and a clone
//! restored from its image, which has the tables but no graph. Each client exports the
//! graph definition from the source, runs it in the clone, mints references there and
//! checks that a reference copied from the source does not resolve in the clone. The
//! connection is plaintext to a listener the test owns.

use crate::driver::{element_counts, round_trip, run, state, DOOR, ROOM_1, ROOM_2, SETUP};
use cassie::app::Cassie;
use cassie::config::CassieRuntimeConfig;
use sqlx::postgres::{PgConnectOptions, PgSslMode};
use sqlx::{ConnectOptions, Connection, Row};
use std::sync::Arc;
use tokio_postgres::{Client, NoTls, SimpleQueryMessage};

const CLONE: &str = "rooms_clone";
const DEFINITION: &str =
    "SELECT property_graph_definition FROM information_schema.property_graphs \
     WHERE property_graph_name = 'rooms'";

struct Served {
    port: u16,
    accept_loop: tokio::task::JoinHandle<()>,
}

/// An in-memory instance with the source graph and a clone of its image without graphs.
async fn serve() -> Served {
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    crate::support_sql::test_root_password().clone_into(&mut config.password);
    let cassie = Cassie::new_in_memory_with_config(config.clone()).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    for sql in SETUP {
        assert!(cassie.execute_sql(&session, sql, vec![]).is_ok());
    }
    let mut backup = cassie.begin_database_backup("postgres").expect("backup");
    let mut image = Vec::new();
    while let Some(chunk) = backup.next_chunk().expect("stream") {
        image.extend(chunk);
    }
    let mut restore = cassie.begin_database_restore(CLONE).expect("restore");
    restore.push_chunk(&image).expect("push");
    restore.finish().expect("finish");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind listener");
    let port = listener.local_addr().expect("listener address").port();
    let shared = Arc::new(cassie);
    let accept_loop = tokio::spawn(async move {
        let _ = cassie::pgwire::server::run_with_listener(listener, shared, config).await;
    });
    Served { port, accept_loop }
}

async fn connect(port: u16, database: &str) -> Client {
    let mut config = tokio_postgres::Config::new();
    config.host("127.0.0.1");
    config.port(port);
    config.user("root");
    config.password(crate::support_sql::test_root_password());
    config.dbname(database);
    let (client, connection) = config.connect(NoTls).await.expect("connect");
    tokio::spawn(async move {
        let _ = connection.await;
    });
    client
}

async fn first_text(client: &Client, sql: &str) -> String {
    let messages = match client.simple_query(sql).await {
        Ok(messages) => messages,
        Err(error) => panic!("query failed with SQLSTATE {}", state(&error)),
    };
    messages
        .into_iter()
        .find_map(|message| match message {
            SimpleQueryMessage::Row(row) => row.get(0).map(str::to_owned),
            _ => None,
        })
        .expect("a text row")
}

async fn failure(client: &Client, sql: &str) -> String {
    match client.simple_query(sql).await {
        Ok(_) => panic!("a statement that should fail succeeded"),
        Err(error) => state(&error),
    }
}

fn path() -> String {
    format!(
        "SELECT ROW(ARRAY[{ROOM_1}, {ROOM_2}], ARRAY[{DOOR}], ARRAY[1::smallint])\
         ::cassie_graph.graph_path"
    )
}

const SOURCE_REFERENCE: &str = "SELECT (cassie_graph.vertex_ref('rooms', 'r', \
     ARRAY[cassie_graph.key_part('t1'::text), cassie_graph.key_part(1::integer)]))::text";

#[test]
fn should_reject_a_graph_reference_in_a_clone_before_the_definition_is_recreated() {
    // Arrange
    let work = async {
        let served = serve().await;
        let clone = connect(served.port, CLONE).await;
        let outcome = failure(&clone, SOURCE_REFERENCE).await;
        served.accept_loop.abort();
        outcome
    };

    // Act
    let outcome = run(work);

    // Assert
    assert_eq!(outcome, "42704");
}

#[test]
fn should_round_trip_a_native_path_in_a_clone_after_recreating_the_exported_definition() {
    // Arrange
    let work = async {
        let served = serve().await;
        let source = connect(served.port, "postgres").await;
        let clone = connect(served.port, CLONE).await;
        let definition = first_text(&source, DEFINITION).await;
        clone.batch_execute(&definition).await.expect("recreate");
        let node = round_trip(&clone, &path(), "graph_path").await;
        served.accept_loop.abort();
        node
    };

    // Act
    let node = run(work);

    // Assert
    assert_eq!(element_counts(node), vec![2, 1, 1]);
}

#[test]
fn should_reject_a_source_reference_copied_to_a_recreated_clone_through_the_driver() {
    // Arrange
    let work = async {
        let served = serve().await;
        let source = connect(served.port, "postgres").await;
        let clone = connect(served.port, CLONE).await;
        let definition = first_text(&source, DEFINITION).await;
        clone.batch_execute(&definition).await.expect("recreate");
        let copied = first_text(&source, SOURCE_REFERENCE).await;
        let call = format!(
            "SELECT * FROM graph_weighted_paths('rooms', '{copied}'::cassie_graph.graph_element_ref, \
             {ROOM_2}, 'weight', 1)"
        );
        let outcome = failure(&clone, &call).await;
        served.accept_loop.abort();
        outcome
    };

    // Act
    let outcome = run(work);

    // Assert
    assert_eq!(outcome, "42704");
}

#[test]
fn should_not_apply_the_exported_definition_twice_through_the_driver() {
    // Arrange
    let work = async {
        let served = serve().await;
        let source = connect(served.port, "postgres").await;
        let clone = connect(served.port, CLONE).await;
        let definition = first_text(&source, DEFINITION).await;
        clone.batch_execute(&definition).await.expect("recreate");
        let outcome = failure(&clone, &definition).await;
        served.accept_loop.abort();
        outcome
    };

    // Act
    let outcome = run(work);

    // Assert
    assert_eq!(outcome, "42710");
}

async fn sqlx_connection(port: u16, database: &str) -> sqlx::PgConnection {
    let options = PgConnectOptions::new()
        .host("127.0.0.1")
        .port(port)
        .username("root")
        .password(crate::support_sql::test_root_password())
        .database(database)
        .ssl_mode(PgSslMode::Disable);
    match options.connect().await {
        Ok(connection) => connection,
        Err(error) => panic!("connect failed: {}", error.as_database_error().is_some()),
    }
}

async fn scalar(connection: &mut sqlx::PgConnection, sql: String) -> String {
    match sqlx::query(sqlx::AssertSqlSafe(sql))
        .fetch_one(&mut *connection)
        .await
    {
        Ok(row) => row.get::<String, _>(0),
        Err(error) => panic!(
            "query failed with SQLSTATE {}",
            error
                .as_database_error()
                .and_then(sqlx::error::DatabaseError::code)
                .map_or_else(|| "none".to_owned(), std::borrow::Cow::into_owned)
        ),
    }
}

#[test]
fn should_carry_clone_local_identities_in_the_json_path_through_sqlx() {
    // Arrange
    let work = async {
        let served = serve().await;
        let mut source = sqlx_connection(served.port, "postgres").await;
        let mut clone = sqlx_connection(served.port, CLONE).await;
        let definition = scalar(&mut source, DEFINITION.to_owned()).await;
        for statement in [definition] {
            sqlx::raw_sql(sqlx::AssertSqlSafe(statement))
                .execute(&mut clone)
                .await
                .expect("recreate");
        }
        let json = format!(
            "SELECT cassie_graph.path_to_jsonb(({}))::text",
            &path()[7..]
        );
        let source_json = scalar(&mut source, json.clone()).await;
        let clone_json = scalar(&mut clone, json).await;
        let _ = source.close().await;
        let _ = clone.close().await;
        served.accept_loop.abort();
        (source_json, clone_json)
    };

    // Act
    let (source_json, clone_json) = run(work);

    // Assert
    let database_of = |text: &str| {
        let value: serde_json::Value = serde_json::from_str(text).expect("json");
        value["vertices"][0]["database_id"]
            .as_str()
            .map(str::to_owned)
    };
    assert!(database_of(&source_json).is_some());
    assert_ne!(database_of(&source_json), database_of(&clone_json));
}

#[test]
fn should_keep_the_clone_path_shape_equal_to_the_source_path_through_sqlx() {
    // Arrange
    let work = async {
        let served = serve().await;
        let mut source = sqlx_connection(served.port, "postgres").await;
        let mut clone = sqlx_connection(served.port, CLONE).await;
        let definition = scalar(&mut source, DEFINITION.to_owned()).await;
        sqlx::raw_sql(sqlx::AssertSqlSafe(definition))
            .execute(&mut clone)
            .await
            .expect("recreate");
        let sql = format!(
            "SELECT s.position::text FROM cassie_graph.path_steps(({})) AS s ORDER BY s.position",
            &path()[7..]
        );
        let (a, b) = (
            scalar(&mut source, sql.clone()).await,
            scalar(&mut clone, sql).await,
        );
        let _ = source.close().await;
        let _ = clone.close().await;
        served.accept_loop.abort();
        (a, b)
    };

    // Act
    let (source_first, clone_first) = run(work);

    // Assert
    assert_eq!(source_first, clone_first);
}
