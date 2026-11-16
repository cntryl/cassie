//! `$n` parameters as `graph_weighted_paths` arguments through Parse, Describe, Bind and
//! Execute, positional and named, in text and binary (#1258).

use crate::support_pgwire as wire;
use crate::support_type_metadata_contract::Frames;
use cassie::app::Cassie;
use cassie::types::Value;

const SETUP: [&str; 5] = [
    "CREATE TABLE v (id BIGINT PRIMARY KEY)",
    "CREATE TABLE fast (id BIGINT PRIMARY KEY, a BIGINT NOT NULL, b BIGINT NOT NULL, weight NUMERIC)",
    "INSERT INTO v VALUES (1), (2), (3)",
    "INSERT INTO fast VALUES (1, 1, 2, 1.5), (2, 2, 3, 2), (3, 1, 3, 4)",
    "CREATE PROPERTY GRAPH wg VERTEX TABLES (v AS n KEY (id) LABEL node PROPERTIES (id)) \
     EDGE TABLES (fast AS f KEY (id) SOURCE KEY (a) REFERENCES n (id) \
     DESTINATION KEY (b) REFERENCES n (id) LABEL fast PROPERTIES (weight))",
];

const ELEMENT_REF_OID_QUERY: &str = "SELECT oid::text FROM pg_type WHERE typname = 'graph_element_ref'";

fn vertex(id: i64) -> String {
    format!("cassie_graph.vertex_ref('wg', 'n', ARRAY[cassie_graph.key_part({id}::bigint)])")
}

struct Rig {
    cassie: Cassie,
    source: String,
    target: String,
    ref_oid: i32,
}

fn rig() -> Rig {
    let mut config = cassie::config::CassieRuntimeConfig::from_env().expect("runtime config");
    crate::support_sql::test_root_password().clone_into(&mut config.password);
    let cassie = Cassie::new_in_memory_with_config(config).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    let run = |sql: &str| match cassie.execute_sql(&session, sql, vec![]) {
        Ok(result) => result.rows().to_vec(),
        Err(error) => panic!("setup failed with SQLSTATE {}", error.sql_state()),
    };
    for sql in SETUP {
        run(sql);
    }
    let text = |rows: Vec<Vec<Value>>| match &rows[0][0] {
        Value::String(text) => text.clone(),
        _ => panic!("expected text"),
    };
    let source = text(run(&format!("SELECT {}::text", vertex(1))));
    let target = text(run(&format!("SELECT {}::text", vertex(3))));
    let ref_oid = text(run(ELEMENT_REF_OID_QUERY)).parse().expect("OID");
    Rig {
        cassie,
        source,
        target,
        ref_oid,
    }
}

fn exchange(cassie: Cassie, cycles: Vec<Vec<Vec<u8>>>) -> Vec<Frames> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let server = wire::spawn_server(cassie).await;
        let socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect pgwire");
        let (mut reader, mut writer) = tokio::io::split(socket);
        wire::complete_startup(&mut reader, &mut writer).await;
        let mut batches = Vec::new();
        for cycle in cycles {
            wire::write_frames(&mut writer, cycle).await;
            batches.push(
                wire::read_frames_until_ready_within(
                    &mut reader,
                    std::time::Duration::from_secs(5),
                )
                .await,
            );
        }
        drop(reader);
        drop(writer);
        server.stop().await;
        batches
    })
}

/// One parameter: bytes, wire format.
type Param = (Option<Vec<u8>>, i16);

fn text(value: &str) -> Param {
    (Some(value.as_bytes().to_vec()), 0)
}

fn int4(value: i32) -> Param {
    (Some(value.to_be_bytes().to_vec()), 1)
}

/// Parse, Describe statement, Bind with `params`, Execute.
fn cycle(sql: &str, params: &[Param]) -> Vec<Vec<u8>> {
    let formats = params.iter().map(|(_, format)| *format).collect::<Vec<_>>();
    let bodies = params
        .iter()
        .map(|(body, _)| body.as_deref())
        .collect::<Vec<_>>();
    vec![
        wire::parse_frame_with_types("", sql, &[]),
        wire::describe_statement_frame(""),
        wire::bind_frame_with_formats("", "", &formats, &bodies, &[]),
        wire::execute_frame(""),
        wire::sync_frame(),
    ]
}

const POSITIONAL: &str = "SELECT p.path_rank::text, p.cost::text FROM graph_weighted_paths(\
    'wg', $1::cassie_graph.graph_element_ref, $2::cassie_graph.graph_element_ref, \
    $3, $4, $5, $6, $7, $8) AS p ORDER BY p.path_rank";

const NAMED: &str = "SELECT p.path_rank::text, p.cost::text FROM graph_weighted_paths(\
    graph => 'wg', source => $1::cassie_graph.graph_element_ref, \
    target => $2::cassie_graph.graph_element_ref, weight_property => $3, max_hops => $4, \
    direction => $5, edge_labels => $6, max_cost => $7, k => $8) AS p ORDER BY p.path_rank";

fn arguments(rig: &Rig, hops: Param, k: Param) -> Vec<Param> {
    vec![
        text(&rig.source),
        text(&rig.target),
        text("weight"),
        hops,
        text("out"),
        text("{fast}"),
        text("10"),
        k,
    ]
}

fn rows_of(frames: &Frames) -> Vec<Vec<Option<String>>> {
    assert_eq!(wire::error_code(frames), None);
    wire::data_rows(frames)
}

fn expected() -> Vec<Vec<Option<String>>> {
    vec![
        vec![Some("1".to_owned()), Some("3.5".to_owned())],
        vec![Some("2".to_owned()), Some("4".to_owned())],
    ]
}

#[test]
fn should_describe_the_weighted_call_parameters_with_the_declared_types() {
    // Arrange
    let rig = rig();
    let reference = rig.ref_oid;
    let params = arguments(&rig, text("2"), text("2"));

    // Act
    let frames = exchange(rig.cassie, vec![cycle(POSITIONAL, &params)]).remove(0);

    // Assert
    let described = frames
        .iter()
        .find(|(tag, _)| *tag == b't')
        .map(|(_, body)| wire::parse_parameter_description(body))
        .expect("parameter description");
    assert_eq!(described, vec![reference, reference, 25, 23, 25, 1009, 1700, 23]);
}

#[test]
fn should_execute_a_positional_call_with_text_parameters() {
    // Arrange
    let rig = rig();
    let params = arguments(&rig, text("2"), text("2"));

    // Act
    let frames = exchange(rig.cassie, vec![cycle(POSITIONAL, &params)]).remove(0);

    // Assert
    assert_eq!(rows_of(&frames), expected());
}

#[test]
fn should_execute_a_positional_call_with_binary_integer_parameters() {
    // Arrange
    let rig = rig();
    let params = arguments(&rig, int4(2), int4(2));

    // Act
    let frames = exchange(rig.cassie, vec![cycle(POSITIONAL, &params)]).remove(0);

    // Assert
    assert_eq!(rows_of(&frames), expected());
}

#[test]
fn should_execute_a_named_call_with_parameters() {
    // Arrange
    let rig = rig();
    let params = arguments(&rig, int4(2), text("2"));

    // Act
    let frames = exchange(rig.cassie, vec![cycle(NAMED, &params)]).remove(0);

    // Assert
    assert_eq!(rows_of(&frames), expected());
}

#[test]
fn should_reject_a_null_parameter_for_a_required_argument_with_22004() {
    // Arrange
    let rig = rig();
    let mut params = arguments(&rig, text("2"), text("2"));
    params[3] = (None, 0);

    // Act
    let frames = exchange(rig.cassie, vec![cycle(POSITIONAL, &params)]).remove(0);

    // Assert
    assert_eq!(wire::error_code(&frames).as_deref(), Some("22004"));
}

#[test]
fn should_reject_a_parameter_that_is_not_a_valid_direction_with_22023() {
    // Arrange
    let rig = rig();
    let mut params = arguments(&rig, text("2"), text("2"));
    params[4] = text("sideways");

    // Act
    let frames = exchange(rig.cassie, vec![cycle(POSITIONAL, &params)]).remove(0);

    // Assert
    assert_eq!(wire::error_code(&frames).as_deref(), Some("22023"));
}
