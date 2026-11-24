//! Projected `GRAPH_TABLE` path variables over the wire: Describe, text and binary results,
//! parameters, and the typed columns of `path_steps` (#1256).

use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;
use cassie::types::Value;

type Setup = Vec<(&'static str, Vec<Value>)>;

const UUID_1: &str = "00000000-0000-0000-0000-000000000001";
const UUID_2: &str = "00000000-0000-0000-0000-000000000002";

fn social() -> Setup {
    crate::fixture::SOCIAL_SETUP
        .iter()
        .map(|sql| (*sql, Vec::new()))
        .collect()
}

fn uuid_world() -> Setup {
    [
        "CREATE TABLE u_node (id UUID PRIMARY KEY)",
        "CREATE TABLE u_link (id BIGINT PRIMARY KEY, src UUID NOT NULL, dst UUID NOT NULL)",
        "INSERT INTO u_node VALUES ('00000000-0000-0000-0000-000000000001'), \
         ('00000000-0000-0000-0000-000000000002')",
        "INSERT INTO u_link VALUES (5, '00000000-0000-0000-0000-000000000001', \
         '00000000-0000-0000-0000-000000000002')",
        "CREATE PROPERTY GRAPH ug VERTEX TABLES (u_node AS un KEY (id) \
           LABEL un PROPERTIES (id)) \
         EDGE TABLES (u_link AS ul KEY (id) SOURCE KEY (src) REFERENCES un (id) \
           DESTINATION KEY (dst) REFERENCES un (id) LABEL ul PROPERTIES (id))",
    ]
    .into_iter()
    .map(|sql| (sql, Vec::new()))
    .collect()
}

fn composite_world() -> Setup {
    [
        "CREATE TABLE c_node (tenant TEXT NOT NULL, seq INT NOT NULL, PRIMARY KEY (tenant, seq))",
        "CREATE TABLE c_link (id BIGINT PRIMARY KEY, st TEXT NOT NULL, ss INT NOT NULL, \
         dt TEXT NOT NULL, ds INT NOT NULL)",
        "INSERT INTO c_node VALUES ('a''b', 1), ('a''b', 2)",
        "INSERT INTO c_link VALUES (5, 'a''b', 1, 'a''b', 2)",
        "CREATE PROPERTY GRAPH cg VERTEX TABLES (c_node AS cn KEY (tenant, seq) \
           LABEL cn PROPERTIES (tenant, seq)) \
         EDGE TABLES (c_link AS cl KEY (id) SOURCE KEY (st, ss) REFERENCES cn (tenant, seq) \
           DESTINATION KEY (dt, ds) REFERENCES cn (tenant, seq) LABEL cl PROPERTIES (id))",
    ]
    .into_iter()
    .map(|sql| (sql, Vec::new()))
    .collect()
}

/// A `graph_path` literal of the given element expressions and direction values.
fn path_literal(vertices: &[String], edges: &[String], directions: &[i16]) -> String {
    let list = |items: &[String]| {
        format!(
            "ARRAY[{}]::cassie_graph.graph_element_ref[]",
            items.join(", ")
        )
    };
    let steps = directions
        .iter()
        .map(|step| format!("{step}::smallint"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "ROW({}, {}, ARRAY[{steps}]::smallint[])::cassie_graph.graph_path",
        list(vertices),
        list(edges)
    )
}

fn reference(kind: &str, graph: &str, mapping: &str, parts: &[String]) -> String {
    format!(
        "cassie_graph.{kind}_ref('{graph}', '{mapping}', ARRAY[{}])",
        parts.join(", ")
    )
}

fn big(number: i64) -> String {
    format!("cassie_graph.key_part({number}::bigint)")
}

fn uuid_part(text: &str) -> String {
    format!("cassie_graph.key_part('{text}'::uuid)")
}

fn composite_part(seq: i32) -> Vec<String> {
    vec![
        "cassie_graph.key_part('a''b'::text)".to_owned(),
        format!("cassie_graph.key_part({seq}::int)"),
    ]
}

fn type_oid(label: &str, name: &str) -> i32 {
    let sql = format!("SELECT oid::text FROM pg_type WHERE typname = '{name}'");
    let cycles = vec![fixture::execute_cycle(&sql, None, 0, false)];
    let batches = fixture::run_cycles(&format!("{label}-oid"), &social(), cycles);
    wire::data_rows(&batches[0])[0][0]
        .as_deref()
        .and_then(|oid| oid.parse().ok())
        .expect("type OID")
}

/// Runs `projected` and `literal` in text and binary result formats, and returns the wire
/// results as `(projected text, literal text, projected binary, literal binary)`.
fn both_forms(label: &str, setup: &Setup, projected: &str, literal: &str) -> Vec<fixture::Frames> {
    let cycles = vec![
        fixture::execute_cycle(projected, None, 0, false),
        fixture::execute_cycle(literal, None, 0, false),
        fixture::execute_cycle(projected, None, 1, false),
        fixture::execute_cycle(literal, None, 1, false),
    ];
    fixture::run_cycles(label, setup, cycles)
}

fn assert_same_wire_values(frames: &[fixture::Frames]) {
    for batch in frames {
        fixture::assert_success(batch);
    }
    let text = wire::data_rows(&frames[0]);
    let binary = fixture::data_row_payloads(&frames[2]);
    assert_eq!(text.len(), 1);
    assert_eq!(binary.len(), 1);
    assert_eq!(text, wire::data_rows(&frames[1]));
    assert_eq!(binary, fixture::data_row_payloads(&frames[3]));
}

#[test]
fn should_describe_a_projected_path_with_the_graph_path_oid() {
    // Arrange
    let sql = "SELECT g.path FROM GRAPH_TABLE (social MATCH p = (a IS person) -[e IS knows]-> \
               (b IS person) COLUMNS (p AS path)) AS g";
    let oid = type_oid("projected-describe", "graph_path");

    // Act
    let text = fixture::run_cycles(
        "projected-describe-text",
        &social(),
        vec![fixture::execute_cycle(sql, None, 0, true)],
    );
    let binary = fixture::run_cycles(
        "projected-describe-binary",
        &social(),
        vec![fixture::execute_cycle(sql, None, 1, true)],
    );

    // Assert
    for (frames, format) in [(&text[0], 0), (&binary[0], 1)] {
        let described = frames
            .iter()
            .filter(|(tag, _)| *tag == b'T')
            .map(|(_, body)| wire::parse_row_description(body))
            .collect::<Vec<_>>();
        assert_eq!(described.len(), 2, "statement and portal descriptions");
        assert_eq!(described[0][0].type_oid, oid);
        assert_eq!(described[1][0].type_oid, oid);
        assert_eq!(described[1][0].format_code, format);
    }
}

#[test]
fn should_send_a_nonempty_projected_path_like_the_equal_literal_in_both_formats() {
    // Arrange
    let projected = "SELECT g.path FROM GRAPH_TABLE (social MATCH \
                     p = (a IS person WHERE a.id = 9007199254740993) -[e IS knows]-> \
                     (b IS person) COLUMNS (p AS path)) AS g";
    let person = |id| reference("vertex", "social", "person", &[big(id)]);
    let literal = format!(
        "SELECT {}",
        path_literal(
            &[person(9_007_199_254_740_993), person(2)],
            &[reference("edge", "social", "knows", &[big(10)])],
            &[1]
        )
    );

    // Act
    let frames = both_forms("projected-nonempty", &social(), projected, &literal);

    // Assert
    assert_same_wire_values(&frames);
}

#[test]
fn should_send_a_zero_hop_projected_path_like_the_equal_literal_in_both_formats() {
    // Arrange
    let projected = "SELECT g.path FROM GRAPH_TABLE (social MATCH \
                     p = (a IS person WHERE a.id = 9007199254740993) COLUMNS (p AS path)) AS g";
    let literal = format!(
        "SELECT {}",
        path_literal(
            &[reference(
                "vertex",
                "social",
                "person",
                &[big(9_007_199_254_740_993)]
            )],
            &[],
            &[]
        )
    );

    // Act
    let frames = both_forms("projected-zero-hop", &social(), projected, &literal);

    // Assert
    assert_same_wire_values(&frames);
}

#[test]
fn should_send_a_uuid_keyed_projected_path_like_the_equal_literal_in_both_formats() {
    // Arrange
    let projected =
        "SELECT g.path FROM GRAPH_TABLE (ug MATCH p = (a IS un) -[e IS ul]-> (b IS un) \
                     COLUMNS (p AS path)) AS g";
    let literal = format!(
        "SELECT {}",
        path_literal(
            &[
                reference("vertex", "ug", "un", &[uuid_part(UUID_1)]),
                reference("vertex", "ug", "un", &[uuid_part(UUID_2)])
            ],
            &[reference("edge", "ug", "ul", &[big(5)])],
            &[1]
        )
    );

    // Act
    let frames = both_forms("projected-uuid", &uuid_world(), projected, &literal);

    // Assert
    assert_same_wire_values(&frames);
}

#[test]
fn should_send_a_composite_keyed_projected_path_like_the_equal_literal_in_both_formats() {
    // Arrange
    let projected =
        "SELECT g.path FROM GRAPH_TABLE (cg MATCH p = (a IS cn) -[e IS cl]-> (b IS cn) \
                     COLUMNS (p AS path)) AS g";
    let literal = format!(
        "SELECT {}",
        path_literal(
            &[
                reference("vertex", "cg", "cn", &composite_part(1)),
                reference("vertex", "cg", "cn", &composite_part(2))
            ],
            &[reference("edge", "cg", "cl", &[big(5)])],
            &[1]
        )
    );

    // Act
    let frames = both_forms(
        "projected-composite",
        &composite_world(),
        projected,
        &literal,
    );

    // Assert
    assert_same_wire_values(&frames);
}

#[test]
fn should_send_a_reverse_projected_path_like_the_equal_literal_in_both_formats() {
    // Arrange
    let projected = "SELECT g.path FROM GRAPH_TABLE (social MATCH \
                     p = (a IS person WHERE a.id = 2) <-[e IS knows]- (b IS person) \
                     COLUMNS (p AS path)) AS g";
    let person = |id| reference("vertex", "social", "person", &[big(id)]);
    let literal = format!(
        "SELECT {}",
        path_literal(
            &[person(2), person(9_007_199_254_740_993)],
            &[reference("edge", "social", "knows", &[big(10)])],
            &[2]
        )
    );

    // Act
    let frames = both_forms("projected-reverse", &social(), projected, &literal);

    // Assert
    assert_same_wire_values(&frames);
}

#[test]
fn should_accept_a_projected_path_text_back_as_a_parameter() {
    // Arrange
    let projected = "SELECT g.path FROM GRAPH_TABLE (social MATCH \
                     p = (a IS person WHERE a.id = 2) <-[e IS knows]- (b IS person) \
                     COLUMNS (p AS path)) AS g";
    let first = fixture::run_cycles(
        "projected-param-a",
        &social(),
        vec![fixture::execute_cycle(projected, None, 0, false)],
    );
    let text = wire::data_rows(&first[0])[0][0].clone().expect("path text");
    let oid = type_oid("projected-param", "graph_path");

    // Act
    let echoed = fixture::run_cycles(
        "projected-param-b",
        &social(),
        vec![fixture::execute_cycle(
            "SELECT cassie_graph.path_to_jsonb($1::cassie_graph.graph_path)",
            Some((oid, 0, Some(text.as_bytes()))),
            0,
            false,
        )],
    );

    // Assert
    fixture::assert_success(&echoed[0]);
    let rows = wire::data_rows(&echoed[0]);
    let document = rows[0][0].clone().expect("jsonb text");
    assert!(document.contains("\"directions\": [2]"));
}

#[test]
fn should_describe_the_path_steps_element_columns_with_the_reference_type() {
    // Arrange
    let sql = format!(
        "SELECT s.vertex, s.edge FROM cassie_graph.path_steps({}) AS s",
        crate::fixture::one_hop(2, 9_007_199_254_740_993, 1)
    );
    let oid = type_oid("path-steps-describe", "graph_element_ref");

    // Act
    let frames = fixture::run_cycles(
        "path-steps-describe",
        &social(),
        vec![fixture::execute_cycle(&sql, None, 0, true)],
    );

    // Assert
    fixture::assert_success(&frames[0]);
    let described = frames[0]
        .iter()
        .filter(|(tag, _)| *tag == b'T')
        .map(|(_, body)| wire::parse_row_description(body))
        .collect::<Vec<_>>();
    assert_eq!(described.len(), 2, "statement and portal descriptions");
    for columns in described {
        let oids = columns
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>();
        assert_eq!(oids, vec![oid, oid]);
    }
}

#[test]
fn should_describe_the_weighted_step_columns_with_typed_element_references() {
    // Arrange
    let sql = format!(
        "SELECT s.vertex, s.edge, s.cumulative_cost \
         FROM cassie_graph.weighted_path_steps({}, ARRAY[1.5]::numeric[]) AS s",
        crate::fixture::one_hop(2, 9_007_199_254_740_993, 1)
    );
    let oid = type_oid("weighted-steps-describe", "graph_element_ref");

    // Act
    let frames = fixture::run_cycles(
        "weighted-steps-describe",
        &social(),
        vec![fixture::execute_cycle(&sql, None, 0, true)],
    );

    // Assert
    fixture::assert_success(&frames[0]);
    let columns = frames[0]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .map(|(_, body)| wire::parse_row_description(body))
        .expect("row description");
    let oids = columns
        .iter()
        .map(|column| column.type_oid)
        .collect::<Vec<_>>();
    assert_eq!(oids, vec![oid, oid, 1700]);
}
