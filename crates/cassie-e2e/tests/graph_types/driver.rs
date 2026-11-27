//! Prepare, describe, bind and execute the reserved graph types through `tokio-postgres`,
//! a real PostgreSQL driver, in binary (prepared) and text (simple query) forms (#1268).
//!
//! The driver has no codec for user-defined composite OIDs, so `record_codec` supplies
//! test-only record and array framing. Binary is the only parameter format the driver sends.

use crate::record_codec::Node;
use cassie::app::Cassie;
use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
use std::sync::Arc;
use tokio_postgres::types::Kind;
use tokio_postgres::{Client, NoTls, SimpleQueryMessage};

pub(crate) const SETUP: [&str; 5] = [
    "CREATE TABLE room (tenant TEXT NOT NULL, seq INT NOT NULL, PRIMARY KEY (tenant, seq))",
    "CREATE TABLE door (id UUID PRIMARY KEY, a_tenant TEXT NOT NULL, a_seq INT NOT NULL, \
     b_tenant TEXT NOT NULL, b_seq INT NOT NULL, weight NUMERIC)",
    "INSERT INTO room VALUES ('t1', 1), ('t1', 2)",
    "INSERT INTO door VALUES ('00000000-0000-0000-0000-0000000000aa', 't1', 1, 't1', 2, 2.5)",
    "CREATE PROPERTY GRAPH rooms VERTEX TABLES (room AS r KEY (tenant, seq) LABEL room \
     PROPERTIES (tenant, seq)) EDGE TABLES (door AS d KEY (id) \
     SOURCE KEY (a_tenant, a_seq) REFERENCES r (tenant, seq) \
     DESTINATION KEY (b_tenant, b_seq) REFERENCES r (tenant, seq) LABEL door PROPERTIES (weight))",
];

pub(crate) const ROOM_1: &str = "cassie_graph.vertex_ref('rooms', 'r', \
    ARRAY[cassie_graph.key_part('t1'::text), cassie_graph.key_part(1::integer)])";
pub(crate) const ROOM_2: &str = "cassie_graph.vertex_ref('rooms', 'r', \
    ARRAY[cassie_graph.key_part('t1'::text), cassie_graph.key_part(2::integer)])";
pub(crate) const DOOR: &str = "cassie_graph.edge_ref('rooms', 'd', \
    ARRAY[cassie_graph.key_part('00000000-0000-0000-0000-0000000000aa'::uuid)])";

struct Server {
    client: Client,
    connection: tokio::task::JoinHandle<()>,
    accept_loop: tokio::task::JoinHandle<()>,
}

/// A running in-memory instance with the setup graph, listening on `port`.
pub(crate) struct Served {
    pub(crate) port: u16,
    pub(crate) accept_loop: tokio::task::JoinHandle<()>,
}

fn driver_config() -> CassieRuntimeConfig {
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    crate::support_sql::test_root_password().clone_into(&mut config.password);
    config
}

/// Starts an in-memory instance, defines the setup graph and serves PostgreSQL wire
/// connections from an owned, bound listener.
pub(crate) async fn serve() -> Served {
    serve_with(&SETUP, driver_config()).await.0
}

async fn serve_with(setup: &[&str], config: CassieRuntimeConfig) -> (Served, Arc<Cassie>) {
    let cassie = Cassie::new_in_memory_with_config(config.clone()).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    for sql in setup {
        if let Err(error) = cassie.execute_sql(&session, sql, vec![]) {
            panic!("setup failed with SQLSTATE {}", error.sql_state());
        }
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind listener");
    let port = listener.local_addr().expect("listener address").port();
    let shared = Arc::new(cassie);
    let served = Arc::clone(&shared);
    let accept_loop = tokio::spawn(async move {
        let _ = cassie::pgwire::server::run_with_listener(listener, served, config).await;
    });
    (Served { port, accept_loop }, shared)
}

async fn start() -> Server {
    connect(serve().await).await
}

async fn connect(Served { port, accept_loop }: Served) -> Server {
    let mut client_config = tokio_postgres::Config::new();
    client_config.host("127.0.0.1");
    client_config.port(port);
    client_config.user("root");
    client_config.password(crate::support_sql::test_root_password());
    client_config.dbname("postgres");
    let (client, connection) = client_config
        .connect(NoTls)
        .await
        .expect("connect tokio-postgres");
    let connection = tokio::spawn(async move {
        let _ = connection.await;
    });
    Server {
        client,
        connection,
        accept_loop,
    }
}

impl Server {
    async fn stop(self) {
        drop(self.client);
        self.connection.abort();
        self.accept_loop.abort();
        let _ = self.connection.await;
        let _ = self.accept_loop.await;
    }
}

pub(crate) fn state(error: &tokio_postgres::Error) -> String {
    error
        .code()
        .map_or_else(|| "none".to_owned(), |code| code.code().to_owned())
}

pub(crate) fn run<T>(work: impl std::future::Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
        .block_on(work)
}

/// The binary node of `expression` through a prepared statement.
async fn binary(client: &Client, sql: &str) -> Node {
    let statement = match client.prepare(sql).await {
        Ok(statement) => statement,
        Err(error) => panic!("prepare failed with SQLSTATE {}", state(&error)),
    };
    match client.query_one(&statement, &[]).await {
        Ok(row) => row.get::<_, Node>(0),
        Err(error) => panic!("execute failed with SQLSTATE {}", state(&error)),
    }
}

/// The text form of `expression` through the simple query protocol.
pub(crate) async fn text(client: &Client, sql: &str) -> String {
    let messages = match client.simple_query(sql).await {
        Ok(messages) => messages,
        Err(error) => panic!("simple query failed with SQLSTATE {}", state(&error)),
    };
    messages
        .into_iter()
        .find_map(|message| match message {
            SimpleQueryMessage::Row(row) => row.get(0).map(str::to_owned),
            _ => None,
        })
        .expect("a text row")
}

/// Binds `node` as a parameter typed by `type_name` and returns its text form, proving the
/// driver-sent binary parameter reaches the server as that type.
async fn echoed(client: &Client, type_name: &str, node: &Node) -> String {
    let statement = match client
        .prepare(&format!("SELECT $1::cassie_graph.{type_name}::text"))
        .await
    {
        Ok(statement) => statement,
        Err(error) => panic!("prepare failed with SQLSTATE {}", state(&error)),
    };
    assert_eq!(
        statement.params()[0].name().trim_start_matches('_'),
        type_name.trim_end_matches("[]")
    );
    match client.query_one(&statement, &[node]).await {
        Ok(row) => row.get::<_, String>(0),
        Err(error) => panic!("bind failed with SQLSTATE {}", state(&error)),
    }
}

/// Binary result, text result and bound echo of `expression` agree.
pub(crate) async fn round_trip(client: &Client, select: &str, type_name: &str) -> Node {
    let node = binary(client, select).await;
    let expected = text(client, select).await;
    assert_eq!(echoed(client, type_name, &node).await, expected);
    node
}

#[test]
fn should_describe_the_graph_types_as_composites_through_the_driver() {
    // Arrange
    let work = async {
        let server = start().await;
        let statement = server
            .client
            .prepare(
                "SELECT cassie_graph.key_part(7::bigint), ARRAY[cassie_graph.key_part(7::bigint)]",
            )
            .await
            .expect("prepare");
        let types = statement
            .columns()
            .iter()
            .map(|column| column.type_().clone())
            .collect::<Vec<_>>();
        server.stop().await;
        types
    };

    // Act
    let types = run(work);

    // Assert
    assert_eq!(types[0].name(), "graph_key_part");
    assert!(matches!(types[0].kind(), Kind::Composite(fields) if fields.len() == 4));
    assert!(matches!(types[1].kind(), Kind::Array(inner) if inner.name() == "graph_key_part"));
}

#[test]
fn should_round_trip_key_parts_of_every_shape_through_the_driver() {
    // Arrange
    let expressions = [
        "cassie_graph.key_part(9007199254740993::bigint)",
        "cassie_graph.key_part('a,b \"c\" (d)'::text)",
        "cassie_graph.key_part('00000000-0000-0000-0000-0000000000aa'::uuid)",
        "cassie_graph.key_part(7::smallint)",
    ];

    // Act
    let nodes = run(async {
        let server = start().await;
        let mut nodes = Vec::new();
        for expression in expressions {
            nodes.push(
                round_trip(
                    &server.client,
                    &format!("SELECT {expression}"),
                    "graph_key_part",
                )
                .await,
            );
        }
        server.stop().await;
        nodes
    });

    // Assert
    let Node::Record(fields) = &nodes[0] else {
        panic!("a record");
    };
    assert_eq!(fields[1], Node::Int8(9_007_199_254_740_993));
    let Node::Record(fields) = &nodes[1] else {
        panic!("a record");
    };
    assert_eq!(fields[2], Node::Text("a,b \"c\" (d)".to_owned()));
}

#[test]
fn should_round_trip_a_key_part_array_with_a_null_element_through_the_driver() {
    // Arrange
    let expression =
        "ARRAY[cassie_graph.key_part(1::bigint), NULL, cassie_graph.key_part(2::bigint)]\
                      ::cassie_graph.graph_key_part[]";

    // Act
    let node = run(async {
        let server = start().await;
        let node = round_trip(
            &server.client,
            &format!("SELECT {expression}"),
            "graph_key_part[]",
        )
        .await;
        server.stop().await;
        node
    });

    // Assert
    let Node::Array(items) = node else {
        panic!("an array");
    };
    assert_eq!(items.len(), 3);
    assert_eq!(items[1], Node::Null);
}

#[test]
fn should_round_trip_a_heterogeneous_composite_key_reference_through_the_driver() {
    // Arrange
    let door_reference = DOOR;

    // Act
    let (vertex, edge) = run(async {
        let server = start().await;
        let vertex = round_trip(
            &server.client,
            &format!("SELECT {ROOM_1}"),
            "graph_element_ref",
        )
        .await;
        let edge = round_trip(
            &server.client,
            &format!("SELECT {door_reference}"),
            "graph_element_ref",
        )
        .await;
        server.stop().await;
        (vertex, edge)
    });

    // Assert
    let Node::Record(fields) = vertex else {
        panic!("a record");
    };
    let Node::Array(parts) = &fields[4] else {
        panic!("key parts");
    };
    assert_eq!(parts.len(), 2);
    let Node::Record(edge_fields) = edge else {
        panic!("a record");
    };
    let Node::Array(edge_parts) = &edge_fields[4] else {
        panic!("key parts");
    };
    assert_eq!(edge_parts.len(), 1);
}

pub(crate) fn element_counts(node: Node) -> Vec<usize> {
    match node {
        Node::Record(fields) => fields
            .iter()
            .map(|field| match field {
                Node::Array(items) => items.len(),
                _ => usize::MAX,
            })
            .collect(),
        _ => Vec::new(),
    }
}

#[test]
fn should_round_trip_a_zero_hop_path_through_the_driver() {
    // Arrange
    let zero = format!(
        "SELECT ROW(ARRAY[{ROOM_1}], ARRAY[]::cassie_graph.graph_element_ref[], \
         ARRAY[]::smallint[])::cassie_graph.graph_path"
    );

    // Act
    let node = run(async {
        let server = start().await;
        let node = round_trip(&server.client, &zero, "graph_path").await;
        server.stop().await;
        node
    });

    // Assert
    assert_eq!(element_counts(node), vec![1, 0, 0]);
}

#[test]
fn should_round_trip_a_one_hop_path_through_the_driver() {
    // Arrange
    let one = format!(
        "SELECT ROW(ARRAY[{ROOM_1}, {ROOM_2}], ARRAY[{DOOR}], ARRAY[1::smallint])\
         ::cassie_graph.graph_path"
    );

    // Act
    let node = run(async {
        let server = start().await;
        let node = round_trip(&server.client, &one, "graph_path").await;
        server.stop().await;
        node
    });

    // Assert
    assert_eq!(element_counts(node), vec![2, 1, 1]);
}

#[test]
fn should_return_equal_paths_from_a_graph_table_read_plus_a_weighted_search_through_the_driver() {
    // Arrange
    let projected = "SELECT g.path FROM GRAPH_TABLE (rooms MATCH \
        p = (a IS room WHERE a.seq = 1) -[e IS door]-> (b IS room) COLUMNS (p AS path)) AS g";
    let weighted = format!(
        "SELECT p.path FROM graph_weighted_paths('rooms', {ROOM_1}, {ROOM_2}, 'weight', 1) AS p"
    );

    // Act
    let (graph_table, search) = run(async {
        let server = start().await;
        let graph_table = round_trip(&server.client, projected, "graph_path").await;
        let search = round_trip(&server.client, &weighted, "graph_path").await;
        server.stop().await;
        (graph_table, search)
    });

    // Assert
    assert_eq!(graph_table, search);
}

#[test]
fn should_report_the_driver_parameter_type_of_a_path_function_argument() {
    // Arrange
    let work = async {
        let server = start().await;
        let statement = server
            .client
            .prepare("SELECT cassie_graph.path_to_jsonb($1::cassie_graph.graph_path)::text")
            .await;
        let name = statement.map(|statement| statement.params()[0].name().to_owned());
        server.stop().await;
        name
    };

    // Act
    let name = run(work);

    // Assert
    assert_eq!(name.expect("prepare succeeds"), "graph_path");
}

#[test]
fn should_probe_the_sparse_weight_index_in_ordinary_pgwire_queries() {
    // Arrange
    let mut config = driver_config();
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    let setup = [
        SETUP[0],
        "CREATE TABLE door (id UUID PRIMARY KEY, a_tenant TEXT NOT NULL, a_seq INT NOT NULL,
         b_tenant TEXT NOT NULL, b_seq INT NOT NULL, weight BIGINT)",
        SETUP[2],
        "INSERT INTO door VALUES ('00000000-0000-0000-0000-0000000000aa', 't1', 1, 't1', 2, 2)",
        "CREATE INDEX door_invalid ON door ((1)) WHERE weight IS NULL OR weight < 0",
        SETUP[4],
    ];
    let sql = format!(
        "SELECT p.cost::text FROM graph_weighted_paths('rooms', {ROOM_1}, {ROOM_2}, 'weight', 1) AS p"
    );

    // Act
    let (costs, validation) = run(async {
        let (served, cassie) = serve_with(&setup, config).await;
        let server = connect(served).await;
        let client = &server.client;
        let counts = || {
            let metrics = cassie.metrics();
            let read = |name: &str| metrics["read_paths"][name].as_u64().expect("counter");
            (
                read("graph_weight_validation_index_probes"),
                read("graph_weight_validation_full_scans"),
            )
        };
        let initial = counts();
        let simple = text(client, &sql).await;
        let after_simple = counts();
        let statement = client.prepare(&sql).await.expect("prepare weighted path");
        let extended = client
            .query_one(&statement, &[])
            .await
            .expect("execute weighted path")
            .get::<_, String>(0);
        let after_extended = counts();
        client.batch_execute("BEGIN").await.expect("begin");
        let explicit = text(client, &sql).await;
        client.batch_execute("ROLLBACK").await.expect("rollback");
        let after_explicit = counts();
        server.stop().await;
        (
            [simple, extended, explicit],
            [initial, after_simple, after_extended, after_explicit],
        )
    });

    // Assert
    assert_eq!(costs.each_ref().map(String::as_str), ["2", "2", "2"]);
    assert_eq!(validation, [(0, 0), (1, 0), (2, 0), (2, 1)]);
}
