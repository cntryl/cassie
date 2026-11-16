//! `RowDescription` and `ParameterDescription` of a bound `GRAPH_TABLE` source.

use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;
use cassie::types::Value;

const BIGINT_OID: i32 = 20;
const TEXT_OID: i32 = 25;
const INT4_OID: i32 = 23;
const BOOL_OID: i32 = 16;

fn setup() -> Vec<(&'static str, Vec<Value>)> {
    vec![
        (
            "CREATE TABLE people (id BIGINT PRIMARY KEY, name TEXT, age INT)",
            Vec::new(),
        ),
        (
            "CREATE TABLE follows (a BIGINT NOT NULL, b BIGINT NOT NULL, active BOOLEAN, \
             CONSTRAINT follows_pk PRIMARY KEY (a, b))",
            Vec::new(),
        ),
        (
            "CREATE PROPERTY GRAPH social
               VERTEX TABLES (people AS person KEY (id) LABEL person PROPERTIES (id, name, age))
               EDGE TABLES (follows AS knows KEY (a, b)
                 SOURCE KEY (a) REFERENCES person (id)
                 DESTINATION KEY (b) REFERENCES person (id)
                 LABEL knows PROPERTIES (active))",
            Vec::new(),
        ),
    ]
}

fn row_descriptions(frames: &fixture::Frames) -> Vec<Vec<wire::RowDescription>> {
    frames
        .iter()
        .filter(|(tag, _)| *tag == b'T')
        .map(|(_, body)| wire::parse_row_description(body))
        .collect()
}

#[test]
fn should_describe_declared_column_types_in_row_description() {
    // Arrange
    let sql = "SELECT g.pid, g.pname, g.years, g.live FROM GRAPH_TABLE (social MATCH \
               (a IS person) -[e IS knows]-> (b IS person) \
               COLUMNS (a.id AS pid, a.name AS pname, a.age AS years, e.active AS live)) AS g";
    let cycles = vec![fixture::execute_cycle(sql, None, 0, true)];

    // Act
    let batches = fixture::run_cycles("graph-table-wire-types", &setup(), cycles);

    // Assert
    let described = row_descriptions(&batches[0]);
    assert_eq!(described.len(), 2, "statement and portal descriptions");
    for columns in &described {
        let shape = columns
            .iter()
            .map(|column| (column.name.as_str(), column.type_oid))
            .collect::<Vec<_>>();
        assert_eq!(
            shape,
            vec![
                ("pid", BIGINT_OID),
                ("pname", TEXT_OID),
                ("years", INT4_OID),
                ("live", BOOL_OID),
            ]
        );
    }
}

#[test]
fn should_describe_a_wildcard_over_a_graph_table() {
    // Arrange
    let sql = "SELECT * FROM GRAPH_TABLE (social MATCH (a IS person) \
               COLUMNS (a.id AS pid, a.name AS pname)) AS g";
    let cycles = vec![fixture::execute_cycle(sql, None, 0, true)];

    // Act
    let batches = fixture::run_cycles("graph-table-wire-wildcard", &setup(), cycles);

    // Assert
    let described = row_descriptions(&batches[0]);
    let shape = described[0]
        .iter()
        .map(|column| (column.name.as_str(), column.type_oid))
        .collect::<Vec<_>>();
    assert_eq!(shape, vec![("pid", BIGINT_OID), ("pname", TEXT_OID)]);
}

#[test]
fn should_infer_a_parameter_type_from_an_inline_predicate() {
    // Arrange
    let sql = "SELECT g.pid FROM GRAPH_TABLE (social MATCH (a IS person WHERE a.id = $1) \
               COLUMNS (a.id AS pid)) AS g";
    let cycles = vec![fixture::execute_cycle(sql, None, 0, true)];

    // Act
    let batches = fixture::run_cycles("graph-table-wire-parameter", &setup(), cycles);

    // Assert
    let parameters = batches[0]
        .iter()
        .find(|(tag, _)| *tag == b't')
        .map(|(_, body)| wire::parse_parameter_description(body))
        .expect("parameter description");
    assert_eq!(parameters, vec![BIGINT_OID]);
}

#[test]
fn should_report_unavailable_execution_after_describing() {
    // Arrange
    let sql = "SELECT g.pid FROM GRAPH_TABLE (social MATCH (a IS person) \
               COLUMNS (a.id AS pid)) AS g";
    let cycles = vec![fixture::execute_cycle(sql, None, 0, true)];

    // Act
    let batches = fixture::run_cycles("graph-table-wire-unavailable", &setup(), cycles);

    // Assert
    assert_eq!(row_descriptions(&batches[0]).len(), 2);
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("0A000"));
}
