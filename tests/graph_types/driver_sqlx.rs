//! Prepare, describe, bind and execute the reserved graph types through `sqlx`, a second
//! real PostgreSQL driver, in binary (prepared) and text (simple query) forms (#1268).
//!
//! `sqlx` resolves user-defined composite and array OIDs through `pg_type`, then
//! `record_codec_sqlx` supplies the record and array framing it has no codec for. Binary is
//! the only parameter format the driver sends, so every value is bound back in binary and
//! compared with its simple-query text form. The connection is plaintext to a local
//! listener owned by the test; the runtime is a current-thread runtime built by hand.

use crate::driver::{run, serve, Served, DOOR, ROOM_1, ROOM_2};
use crate::record_codec_sqlx::{Bound, TextForm};
use crate::record_frame::Node;
use sqlx::postgres::{PgConnectOptions, PgSslMode, PgTypeKind};
use sqlx::{
    query, raw_sql, AssertSqlSafe, Column, ConnectOptions, Connection, Either, Executor,
    PgConnection, Row, SqlSafeStr, Statement, TypeInfo,
};

struct Server {
    connection: PgConnection,
    accept_loop: tokio::task::JoinHandle<()>,
}

/// The SQLSTATE of a failed step, or `none` when the failure is not a database error.
fn state(error: &sqlx::Error) -> String {
    error
        .as_database_error()
        .and_then(|database| database.code())
        .map_or_else(|| "none".to_owned(), |code| code.into_owned())
}

/// The value of a step that must succeed; a failure reports only its step and SQLSTATE.
fn must<T>(outcome: Result<T, sqlx::Error>, step: &str) -> T {
    match outcome {
        Ok(value) => value,
        Err(error) => panic!("{step} failed with SQLSTATE {}", state(&error)),
    }
}

async fn start() -> Server {
    let Served { port, accept_loop } = serve().await;
    let options = PgConnectOptions::new()
        .host("127.0.0.1")
        .port(port)
        .username("root")
        .password(crate::support_sql::test_root_password())
        .database("postgres")
        .ssl_mode(PgSslMode::Disable);
    let connection = must(options.connect().await, "connect");
    Server {
        connection,
        accept_loop,
    }
}

impl Server {
    async fn stop(self) {
        let _ = self.connection.close().await;
        self.accept_loop.abort();
        let _ = self.accept_loop.await;
    }
}

/// The binary node of `sql` through a prepared statement.
async fn binary(connection: &mut PgConnection, sql: &str) -> Node {
    let row = must(
        query(AssertSqlSafe(sql.to_owned()))
            .fetch_one(&mut *connection)
            .await,
        "execute",
    );
    must(row.try_get::<Node, _>(0), "decode")
}

/// The text form of `sql` through the simple query protocol.
async fn text(connection: &mut PgConnection, sql: &str) -> String {
    let rows = must(
        raw_sql(AssertSqlSafe(sql.to_owned()))
            .fetch_all(&mut *connection)
            .await,
        "simple query",
    );
    must(rows[0].try_get::<TextForm, _>(0), "decode text").0
}

/// Binds `node` as a parameter typed by `type_name` and returns its text form, proving the
/// driver-sent binary parameter reaches the server as that type.
async fn echoed(connection: &mut PgConnection, type_name: &str, node: &Node) -> String {
    let sql = format!("SELECT $1::cassie_graph.{type_name}::text");
    let statement = must(
        (&mut *connection)
            .prepare(AssertSqlSafe(sql.clone()).into_sql_str())
            .await,
        "prepare",
    );
    let Some(Either::Left(types)) = statement.parameters() else {
        panic!("the statement reports its parameter types");
    };
    let info = types[0].clone();
    assert_eq!(
        info.name().trim_start_matches('_'),
        type_name.trim_end_matches("[]")
    );
    let bound = Bound {
        node: node.clone(),
        info,
    };
    let row = must(
        query(AssertSqlSafe(sql))
            .bind(bound)
            .fetch_one(&mut *connection)
            .await,
        "bind",
    );
    must(row.try_get::<String, _>(0), "decode")
}

/// Binary result, text result and bound echo of `select` agree.
async fn round_trip(connection: &mut PgConnection, select: &str, type_name: &str) -> Node {
    let node = binary(connection, select).await;
    let expected = text(connection, select).await;
    assert_eq!(echoed(connection, type_name, &node).await, expected);
    node
}

fn zero_hop() -> String {
    format!(
        "ROW(ARRAY[{ROOM_1}], ARRAY[]::cassie_graph.graph_element_ref[], \
         ARRAY[]::smallint[])::cassie_graph.graph_path"
    )
}

fn one_hop() -> String {
    format!(
        "ROW(ARRAY[{ROOM_1}, {ROOM_2}], ARRAY[{DOOR}], ARRAY[1::smallint])\
         ::cassie_graph.graph_path"
    )
}

fn element_counts(node: Node) -> Vec<usize> {
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
fn should_describe_the_graph_types_as_composites_through_sqlx() {
    // Arrange
    let work = async {
        let mut server = start().await;
        let statement = must(
            (&mut server.connection)
                .prepare(
                    AssertSqlSafe(
                        "SELECT cassie_graph.key_part(7::bigint), \
                         ARRAY[cassie_graph.key_part(7::bigint)]"
                            .to_owned(),
                    )
                    .into_sql_str(),
                )
                .await,
            "prepare",
        );
        let types = statement
            .columns()
            .iter()
            .map(|column| column.type_info().clone())
            .collect::<Vec<_>>();
        server.stop().await;
        types
    };

    // Act
    let types = run(work);

    // Assert
    assert_eq!(types[0].name(), "graph_key_part");
    assert!(matches!(types[0].kind(), PgTypeKind::Composite(fields) if fields.len() == 4));
    assert!(
        matches!(types[1].kind(), PgTypeKind::Array(inner) if inner.name() == "graph_key_part")
    );
}

#[test]
fn should_round_trip_key_parts_of_every_shape_through_sqlx() {
    // Arrange
    let expressions = [
        "cassie_graph.key_part(9007199254740993::bigint)",
        "cassie_graph.key_part('a,b \"c\" (d)'::text)",
        "cassie_graph.key_part('00000000-0000-0000-0000-0000000000aa'::uuid)",
        "cassie_graph.key_part(7::smallint)",
    ];

    // Act
    let nodes = run(async {
        let mut server = start().await;
        let mut nodes = Vec::new();
        for expression in expressions {
            nodes.push(
                round_trip(
                    &mut server.connection,
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
    let Node::Record(fields) = &nodes[3] else {
        panic!("a record");
    };
    assert!(matches!(fields[0], Node::Int2(_)));
    assert_eq!(fields[1], Node::Int8(7));
}

#[test]
fn should_round_trip_a_key_part_array_with_a_null_element_through_sqlx() {
    // Arrange
    let expression =
        "ARRAY[cassie_graph.key_part(1::bigint), NULL, cassie_graph.key_part(2::bigint)]\
         ::cassie_graph.graph_key_part[]";

    // Act
    let node = run(async {
        let mut server = start().await;
        let node = round_trip(
            &mut server.connection,
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
fn should_round_trip_a_heterogeneous_composite_key_reference_through_sqlx() {
    // Arrange
    let door_reference = DOOR;

    // Act
    let (vertex, edge) = run(async {
        let mut server = start().await;
        let vertex = round_trip(
            &mut server.connection,
            &format!("SELECT {ROOM_1}"),
            "graph_element_ref",
        )
        .await;
        let edge = round_trip(
            &mut server.connection,
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

#[test]
fn should_round_trip_a_zero_hop_path_through_sqlx() {
    // Arrange
    let zero = format!("SELECT {}", zero_hop());

    // Act
    let node = run(async {
        let mut server = start().await;
        let node = round_trip(&mut server.connection, &zero, "graph_path").await;
        server.stop().await;
        node
    });

    // Assert
    assert_eq!(element_counts(node), vec![1, 0, 0]);
}

#[test]
fn should_round_trip_a_one_hop_path_through_sqlx() {
    // Arrange
    let one = format!("SELECT {}", one_hop());

    // Act
    let node = run(async {
        let mut server = start().await;
        let node = round_trip(&mut server.connection, &one, "graph_path").await;
        server.stop().await;
        node
    });

    // Assert
    assert_eq!(element_counts(node), vec![2, 1, 1]);
}

#[test]
fn should_round_trip_an_array_of_element_references_through_sqlx() {
    // Arrange
    let references = format!("SELECT ARRAY[{ROOM_1}, {ROOM_2}]::cassie_graph.graph_element_ref[]");

    // Act
    let node = run(async {
        let mut server = start().await;
        let node = round_trip(&mut server.connection, &references, "graph_element_ref[]").await;
        server.stop().await;
        node
    });

    // Assert
    let Node::Array(items) = node else {
        panic!("an array");
    };
    assert_eq!(items.len(), 2);
}

#[test]
fn should_round_trip_an_array_of_paths_with_a_null_element_through_sqlx() {
    // Arrange
    let paths = format!(
        "SELECT ARRAY[{}, NULL, {}]::cassie_graph.graph_path[]",
        zero_hop(),
        one_hop()
    );

    // Act
    let node = run(async {
        let mut server = start().await;
        let node = round_trip(&mut server.connection, &paths, "graph_path[]").await;
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
fn should_return_equal_paths_from_a_graph_table_read_plus_a_weighted_search_through_sqlx() {
    // Arrange
    let projected = "SELECT g.path FROM GRAPH_TABLE (rooms MATCH \
        p = (a IS room WHERE a.seq = 1) -[e IS door]-> (b IS room) COLUMNS (p AS path)) AS g";
    let weighted = format!(
        "SELECT p.path FROM graph_weighted_paths('rooms', {ROOM_1}, {ROOM_2}, 'weight', 1) AS p"
    );

    // Act
    let (graph_table, search) = run(async {
        let mut server = start().await;
        let graph_table = round_trip(&mut server.connection, projected, "graph_path").await;
        let search = round_trip(&mut server.connection, &weighted, "graph_path").await;
        server.stop().await;
        (graph_table, search)
    });

    // Assert
    assert_eq!(graph_table, search);
}

/// The reported name of the first parameter type of `sql`.
async fn parameter_name(connection: &mut PgConnection, sql: &str) -> String {
    let statement = must(
        (&mut *connection)
            .prepare(AssertSqlSafe(sql.to_owned()).into_sql_str())
            .await,
        "prepare",
    );
    let Some(Either::Left(types)) = statement.parameters() else {
        panic!("the statement reports its parameter types");
    };
    types[0].name().to_owned()
}

#[test]
fn should_report_the_driver_parameter_type_of_a_path_function_argument() {
    // Arrange
    let sql = "SELECT cassie_graph.path_to_jsonb($1::cassie_graph.graph_path)::text";

    // Act
    let name = run(async {
        let mut server = start().await;
        let name = parameter_name(&mut server.connection, sql).await;
        server.stop().await;
        name
    });

    // Assert
    assert_eq!(name, "graph_path");
}

#[test]
fn should_report_the_inferred_parameter_type_of_an_undeclared_path_function_argument() {
    // Arrange
    let sql = "SELECT cassie_graph.path_to_jsonb($1)::text";

    // Act
    let name = run(async {
        let mut server = start().await;
        let name = parameter_name(&mut server.connection, sql).await;
        server.stop().await;
        name
    });

    // Assert
    assert_eq!(name, "graph_path");
}
