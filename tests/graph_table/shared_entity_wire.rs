//! The shared-entity join through prepared statements over pgwire (#1353): the permission
//! example, a common-neighbor count, tenant separation and own-write visibility.

use crate::shared_entity_fixture::setup;
use crate::support_pgwire as wire;
use crate::support_type_metadata_contract::{run_cycles, Frames};

const TEXT_OID: i32 = 25;
const BIGINT_OID: i32 = 20;

const PERMISSION: &str = "SELECT g.rev_id, g.doc_id FROM GRAPH_TABLE (social MATCH \
     (req IS person WHERE req.tenant = $1 AND req.id = $2) -[IS knows]->{1,2} (rev IS person), \
     (rev) -[IS owns]-> (d IS document) \
     COLUMNS (rev.id AS rev_id, d.id AS doc_id)) AS g ORDER BY g.rev_id, g.doc_id";

const COUNT: &str = "SELECT COUNT(*) FROM GRAPH_TABLE (social MATCH \
     (a IS person) -[IS knows]-> (n IS person), (b IS person) -[IS knows]-> (n) \
     WHERE n.tenant = $1 AND a.id < b.id COLUMNS (n.id AS n)) AS g";

fn prepare(name: &str, sql: &str, types: &[i32]) -> Vec<Vec<u8>> {
    vec![
        wire::parse_frame_with_types(name, sql, types),
        wire::describe_statement_frame(name),
        wire::sync_frame(),
    ]
}

fn execute(name: &str, params: &[&str]) -> Vec<Vec<u8>> {
    let formats = vec![0; params.len()];
    let bodies = params
        .iter()
        .map(|param| Some(param.as_bytes()))
        .collect::<Vec<_>>();
    vec![
        wire::bind_frame_with_formats("", name, &formats, &bodies, &[]),
        wire::execute_frame(""),
        wire::sync_frame(),
    ]
}

fn simple(sql: &str) -> Vec<Vec<u8>> {
    vec![wire::simple_query_frame(sql)]
}

fn rows(frames: &Frames) -> Vec<Vec<String>> {
    assert_eq!(wire::error_code(frames), None);
    wire::data_rows(frames)
        .into_iter()
        .map(|row| row.into_iter().map(|cell| cell.expect("a value")).collect())
        .collect()
}

fn texts(rows: &[&[&str]]) -> Vec<Vec<String>> {
    rows.iter()
        .map(|row| row.iter().map(|cell| (*cell).to_owned()).collect())
        .collect()
}

#[test]
fn should_describe_the_parameters_of_both_patterns() {
    // Arrange
    let cycles = vec![prepare("permission", PERMISSION, &[])];

    // Act
    let batches = run_cycles("graph-shared-entity-describe", &setup(), cycles);

    // Assert
    let parameters = batches[0]
        .iter()
        .find(|(tag, _)| *tag == b't')
        .map(|(_, body)| wire::parse_parameter_description(body))
        .expect("parameter description");
    assert_eq!(parameters, vec![TEXT_OID, BIGINT_OID]);
}

#[test]
fn should_answer_the_permission_example_for_each_tenant_through_one_prepared_statement() {
    // Arrange
    let cycles = vec![
        prepare("permission", PERMISSION, &[]),
        execute("permission", &["a", "1"]),
        execute("permission", &["b", "1"]),
        execute("permission", &["c", "1"]),
    ];

    // Act
    let batches = run_cycles("graph-shared-entity-permission", &setup(), cycles);

    // Assert
    assert_eq!(
        rows(&batches[1]),
        texts(&[
            &["3", "1"],
            &["3", "1"],
            &["5", "2"],
            &["5", "2"],
            &["5", "2"]
        ])
    );
    assert_eq!(
        rows(&batches[2]),
        texts(&[&["2", "2"], &["3", "1"], &["3", "1"]])
    );
    assert_eq!(rows(&batches[3]), Vec::<Vec<String>>::new());
}

#[test]
fn should_count_the_common_neighbors_of_each_tenant_separately() {
    // Arrange
    let cycles = vec![
        prepare("common", COUNT, &[]),
        execute("common", &["a"]),
        execute("common", &["b"]),
    ];

    // Act
    let batches = run_cycles("graph-shared-entity-count", &setup(), cycles);

    // Assert
    assert_eq!(rows(&batches[1]), texts(&[&["4"]]));
    assert_eq!(rows(&batches[2]), texts(&[&["1"]]));
}

#[test]
fn should_discard_own_writes_on_rollback_through_a_prepared_statement() {
    // Arrange
    let owner = "SELECT g.p FROM GRAPH_TABLE (social MATCH \
         (a IS person) -[IS knows]-> (n IS person), \
         (b IS person WHERE b.tenant = $1 AND b.id = 6) -[IS knows]-> (n) \
         COLUMNS (a.id AS p)) AS g ORDER BY g.p";
    let cycles = vec![
        prepare("owner", owner, &[]),
        execute("owner", &["a"]),
        simple("BEGIN"),
        simple("INSERT INTO person VALUES ('a', 6, 'fay')"),
        simple("INSERT INTO knows VALUES (9, 'a', 6, 'a', 3)"),
        execute("owner", &["a"]),
        simple("ROLLBACK"),
        execute("owner", &["a"]),
    ];

    // Act
    let batches = run_cycles("graph-shared-entity-own-writes", &setup(), cycles);

    // Assert
    assert_eq!(rows(&batches[1]), Vec::<Vec<String>>::new());
    assert_eq!(rows(&batches[5]), texts(&[&["1"], &["1"], &["2"], &["6"]]));
    assert_eq!(rows(&batches[7]), Vec::<Vec<String>>::new());
}
