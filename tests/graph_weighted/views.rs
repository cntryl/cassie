//! The statement's own view, the typed result and its composition with the path functions.

use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;
use crate::world::{fast, vertex, Wg};
use cassie::types::Value;

const BIGINT_OID: i32 = 20;
const NUMERIC_OID: i32 = 1700;
const NUMERIC_ARRAY_OID: i32 = 1231;

#[test]
fn should_see_an_own_uncommitted_edge_and_hide_it_from_another_session() {
    // Arrange
    let world = Wg::new(3, &[fast(1, 1, 2, 1)]);
    let reader = world.fixture.cassie.create_session("tester", None);
    let sql = Wg::select(&format!("'wg', {}, {}, 'weight', 2", vertex(1), vertex(3)));
    world.run("BEGIN");
    world.run("INSERT INTO fast VALUES (2, 2, 3, 4, 'x', 1.5)");

    // Act
    let inside = world.call(1, 3, "'weight', 2").expect("call succeeds");
    let outside = match world.fixture.cassie.execute_sql(&reader, &sql, vec![]) {
        Ok(result) => result.rows().len(),
        Err(error) => panic!("reader failed with SQLSTATE {}", error.sql_state()),
    };

    // Assert
    assert_eq!((inside.len(), inside[0].cost, outside), (1, 5_000_000, 0));
}

#[test]
fn should_discard_an_uncommitted_edge_on_rollback() {
    // Arrange
    let world = Wg::new(3, &[fast(1, 1, 2, 1)]);
    world.run("BEGIN");
    world.run("INSERT INTO fast VALUES (2, 2, 3, 4, 'x', 1.5)");
    world.run("ROLLBACK");

    // Act
    let rows = world.call(1, 3, "'weight', 2").expect("call succeeds");

    // Assert
    assert!(rows.is_empty());
}

#[test]
fn should_feed_the_result_to_weighted_path_steps() {
    // Arrange
    let world = Wg::new(3, &[fast(1, 1, 2, 1), fast(2, 2, 3, 2)]);
    let sql = format!(
        "SELECT s.position, s.cumulative_cost::text FROM graph_weighted_paths('wg', {}, {}, \
         'weight', 2) AS p CROSS JOIN LATERAL cassie_graph.weighted_path_steps(p.path, p.edge_costs) \
         AS s ORDER BY s.position",
        vertex(1),
        vertex(3)
    );

    // Act
    let rows = world.fixture.rows(&sql);

    // Assert
    let costs = rows
        .iter()
        .map(|row| match &row[1] {
            Value::String(text) => text.clone(),
            _ => panic!("expected text"),
        })
        .collect::<Vec<_>>();
    assert_eq!(costs, vec!["0", "1.000000", "3.000000"]);
}

#[test]
fn should_feed_the_result_to_path_to_jsonb() {
    // Arrange
    let world = Wg::new(2, &[fast(1, 1, 2, 3)]);
    let sql = format!(
        "SELECT cassie_graph.path_to_jsonb(p.path)->>'version', \
         jsonb_array_length(cassie_graph.path_to_jsonb(p.path)->'edges') \
         FROM graph_weighted_paths('wg', {}, {}, 'weight', 1) AS p",
        vertex(1),
        vertex(2)
    );

    // Act
    let rows = world.fixture.rows(&sql);

    // Assert
    assert_eq!(rows, vec![vec![Value::String("1".to_owned()), Value::Int64(1)]]);
}

#[test]
fn should_return_every_row_in_rank_order_with_matching_path_lengths() {
    // Arrange
    let world = Wg::new(3, &[fast(1, 1, 2, 1), fast(2, 2, 3, 1), fast(3, 1, 3, 2)]);

    // Act
    let rows = world.call(1, 3, "'weight', 2, 'out', NULL, NULL, 2").expect("call succeeds");

    // Assert
    let ranks = rows.iter().map(|row| row.rank).collect::<Vec<_>>();
    assert_eq!(ranks, vec![1, 2]);
    for row in &rows {
        assert_eq!(row.edge_costs.len(), row.edges.len());
        assert_eq!(row.edge_costs.iter().sum::<i128>(), row.cost);
    }
}

fn payload_text(payload: &[u8]) -> String {
    String::from_utf8(payload[6..].to_vec()).expect("text field")
}

#[test]
fn should_describe_the_typed_result_columns() {
    // Arrange
    let setup = vec![
        ("CREATE TABLE v (id BIGINT PRIMARY KEY)", Vec::new()),
        (
            "CREATE TABLE e (id BIGINT PRIMARY KEY, a BIGINT NOT NULL, b BIGINT NOT NULL, w NUMERIC)",
            Vec::new(),
        ),
        ("INSERT INTO v VALUES (1), (2)", Vec::new()),
        ("INSERT INTO e VALUES (1, 1, 2, 3)", Vec::new()),
        (
            "CREATE PROPERTY GRAPH wg VERTEX TABLES (v AS n KEY (id) LABEL node PROPERTIES (id)) \
             EDGE TABLES (e AS l KEY (id) SOURCE KEY (a) REFERENCES n (id) \
             DESTINATION KEY (b) REFERENCES n (id) LABEL link PROPERTIES (w))",
            Vec::new(),
        ),
    ];
    let reference = |id: i64| {
        format!("cassie_graph.vertex_ref('wg', 'n', ARRAY[cassie_graph.key_part({id}::bigint)])")
    };
    let sql = format!(
        "SELECT * FROM graph_weighted_paths('wg', {}, {}, 'w', 1) AS p",
        reference(1),
        reference(2)
    );
    let type_sql = "SELECT oid FROM pg_catalog.pg_type WHERE typname = 'graph_path'";
    let cycles = vec![
        fixture::execute_cycle(&sql, None, 0, true),
        fixture::execute_cycle(type_sql, None, 0, true),
    ];

    // Act
    let batches = fixture::run_cycles("graph-weighted-wire-types", &setup, cycles);

    // Assert
    let described = batches[0]
        .iter()
        .filter(|(tag, _)| *tag == b'T')
        .map(|(_, body)| wire::parse_row_description(body))
        .collect::<Vec<_>>();
    let path_oid = payload_text(&fixture::data_row_payloads(&batches[1])[0])
        .parse::<i32>()
        .expect("oid text");
    assert_eq!(described.len(), 2, "statement and portal descriptions");
    for columns in &described {
        let shape = columns
            .iter()
            .map(|column| (column.name.as_str(), column.type_oid))
            .collect::<Vec<_>>();
        assert_eq!(
            shape,
            vec![
                ("path_rank", BIGINT_OID),
                ("cost", NUMERIC_OID),
                ("path", path_oid),
                ("edge_costs", NUMERIC_ARRAY_OID),
            ]
        );
    }
}
