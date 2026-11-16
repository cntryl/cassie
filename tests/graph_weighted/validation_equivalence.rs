//! Cases of the streaming weight validation that the shared oracle world cannot express:
//! NULL endpoint components, composite keys, the in-full fallback of a parallel scan and
//! quoted mixed-case columns. Every case runs under the four read paths (lazy without and
//! with endpoint indexes, exact without and with them) and must answer identically (#1258,
//! #1268). Every test here is authored/not run.
//!
//! `Endpoints::scan_keys` (a vertex mapping with no usable key index) is reached through a
//! REPEATABLE READ transaction opened before a second unique key replaced the original: the
//! replacement is not visible to that transaction and `find_index` returns None
//! (`transaction_index_visible`). `should_answer_the_same_when_a_repeatable_read_cannot_see_the_replacing_key_index`
//! covers it with two sessions.

use crate::world::{started, Variant, VariantGuard};
use cassie::config::CassieRuntimeConfig;
use cassie::types::Value;

const VARIANTS: [Variant; 4] = [
    Variant {
        exact: false,
        endpoint_indexes: false,
    },
    Variant {
        exact: false,
        endpoint_indexes: true,
    },
    Variant {
        exact: true,
        endpoint_indexes: false,
    },
    Variant {
        exact: true,
        endpoint_indexes: true,
    },
];

/// One world: tables and rows, the indexes only the indexed variants create, the graph, and
/// the call whose answer is compared.
struct Case {
    tables: Vec<&'static str>,
    indexes: Vec<&'static str>,
    graph: &'static str,
    call: String,
    workers: usize,
}

/// The rank, cost and edge costs of each answer row, or the SQLSTATE of a failure.
type Answer = Result<Vec<Vec<String>>, String>;

fn cell(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Int64(number) => number.to_string(),
        _ => panic!("expected text or integer"),
    }
}

fn answer(case: &Case, variant: Variant) -> Answer {
    let _guard = VariantGuard::set(variant);
    let mut config = crate::support_sql::with_test_root_password(
        CassieRuntimeConfig::from_env().expect("runtime config"),
    );
    config.limits.parallel_scan_workers = case.workers;
    let mut setup = case.tables.clone();
    if variant.endpoint_indexes {
        setup.extend(&case.indexes);
    }
    setup.push(case.graph);
    let fixture = started(Some(config), &setup);
    let sql = format!(
        "SELECT p.path_rank::text, p.cost::text, p.edge_costs::text \
         FROM graph_weighted_paths({}) AS p ORDER BY p.path_rank",
        case.call
    );
    match fixture.execute(&sql) {
        Ok(result) => Ok(result
            .rows()
            .iter()
            .map(|row| row.iter().map(cell).collect())
            .collect()),
        Err(error) => Err(error.sql_state().to_owned()),
    }
}

fn answers(case: &Case) -> Vec<Answer> {
    VARIANTS
        .into_iter()
        .map(|variant| answer(case, variant))
        .collect()
}

fn ok(rows: &[&[&str]]) -> Answer {
    Ok(rows
        .iter()
        .map(|row| row.iter().map(|text| (*text).to_owned()).collect())
        .collect())
}

fn nullable_endpoint(workers: usize, extra_edge: &'static str) -> Case {
    Case {
        tables: vec![
            "CREATE TABLE pn (id BIGINT PRIMARY KEY)",
            "CREATE TABLE en (id BIGINT PRIMARY KEY, s BIGINT, d BIGINT, weight NUMERIC)",
            "INSERT INTO pn VALUES (1), (2), (3)",
            "INSERT INTO en VALUES (1, 1, 2, 1), (2, 2, 3, 1)",
            extra_edge,
        ],
        indexes: vec!["CREATE INDEX en_s ON en (s)", "CREATE INDEX en_d ON en (d)"],
        graph: "CREATE PROPERTY GRAPH ng VERTEX TABLES (pn AS n KEY (id) LABEL node \
                PROPERTIES (id)) EDGE TABLES (en AS e KEY (id) SOURCE KEY (s) \
                REFERENCES n (id) DESTINATION KEY (d) REFERENCES n (id) LABEL link \
                PROPERTIES (weight))",
        call: "'ng', cassie_graph.vertex_ref('ng', 'n', ARRAY[cassie_graph.key_part(1::bigint)]), \
               cassie_graph.vertex_ref('ng', 'n', ARRAY[cassie_graph.key_part(3::bigint)]), \
               'weight', 2"
            .to_owned(),
        workers,
    }
}

#[test]
fn should_ignore_an_invalid_weight_on_an_edge_with_a_null_endpoint_under_every_read_path() {
    // Arrange
    let case = nullable_endpoint(1, "INSERT INTO en VALUES (3, NULL, 3, -1)");

    // Act
    let all = answers(&case);

    // Assert
    let expected = ok(&[&["1", "2", "{1,1}"]]);
    assert!(all.iter().all(|found| *found == expected));
}

#[test]
fn should_reject_the_invalid_weight_of_a_visible_edge_beside_a_null_endpoint_edge_under_every_read_path(
) {
    // Arrange
    let case = nullable_endpoint(1, "INSERT INTO en VALUES (3, 1, 3, -1), (4, NULL, 3, -1)");

    // Act
    let all = answers(&case);

    // Assert
    assert!(all.iter().all(|found| *found == Err("22023".to_owned())));
}

#[test]
fn should_answer_the_same_when_the_parallel_scan_falls_back_to_reading_in_full() {
    // Arrange
    let sequential = nullable_endpoint(1, "INSERT INTO en VALUES (3, NULL, 3, -1)");
    let parallel = nullable_endpoint(2, "INSERT INTO en VALUES (3, NULL, 3, -1)");

    // Act
    let (one, two) = (answers(&sequential), answers(&parallel));

    // Assert
    assert_eq!(one, two);
    assert_eq!(two[0], ok(&[&["1", "2", "{1,1}"]]));
}

#[test]
fn should_reject_an_invalid_visible_weight_when_the_parallel_scan_reads_in_full() {
    // Arrange
    let case = nullable_endpoint(2, "INSERT INTO en VALUES (3, 1, 3, -1)");

    // Act
    let all = answers(&case);

    // Assert
    assert!(all.iter().all(|found| *found == Err("22023".to_owned())));
}

fn composite(extra_edge: &'static str) -> Case {
    Case {
        tables: vec![
            "CREATE TABLE cn (k1 BIGINT NOT NULL, k2 TEXT NOT NULL, PRIMARY KEY (k1, k2))",
            "CREATE TABLE ce (id BIGINT PRIMARY KEY, s1 BIGINT, s2 TEXT, d1 BIGINT, d2 TEXT, \
             weight NUMERIC)",
            "INSERT INTO cn VALUES (1, 'a'), (2, 'b'), (3, 'c')",
            "INSERT INTO ce VALUES (1, 1, 'a', 2, 'b', 1), (2, 2, 'b', 3, 'c', 1)",
            extra_edge,
        ],
        indexes: vec![
            "CREATE INDEX ce_s ON ce (s1, s2)",
            "CREATE INDEX ce_d ON ce (d1, d2)",
        ],
        graph: "CREATE PROPERTY GRAPH cg VERTEX TABLES (cn AS n KEY (k1, k2) LABEL node \
                PROPERTIES (k1, k2)) EDGE TABLES (ce AS e KEY (id) SOURCE KEY (s1, s2) \
                REFERENCES n (k1, k2) DESTINATION KEY (d1, d2) REFERENCES n (k1, k2) \
                LABEL link PROPERTIES (weight))",
        call: "'cg', cassie_graph.vertex_ref('cg', 'n', ARRAY[cassie_graph.key_part(1::bigint), \
               cassie_graph.key_part('a'::text)]), \
               cassie_graph.vertex_ref('cg', 'n', ARRAY[cassie_graph.key_part(3::bigint), \
               cassie_graph.key_part('c'::text)]), 'weight', 2"
            .to_owned(),
        workers: 1,
    }
}

#[test]
fn should_ignore_an_invalid_weight_toward_a_missing_composite_vertex_under_every_read_path() {
    // Arrange
    let case = composite("INSERT INTO ce VALUES (3, 1, 'a', 9, 'z', -1)");

    // Act
    let all = answers(&case);

    // Assert
    let expected = ok(&[&["1", "2", "{1,1}"]]);
    assert!(all.iter().all(|found| *found == expected));
}

#[test]
fn should_reject_an_invalid_weight_between_two_existing_composite_vertices_under_every_read_path() {
    // Arrange
    let case = composite("INSERT INTO ce VALUES (3, 1, 'a', 3, 'c', -1)");

    // Act
    let all = answers(&case);

    // Assert
    assert!(all.iter().all(|found| *found == Err("22023".to_owned())));
}

fn quoted(extra_edge: &'static str) -> Case {
    Case {
        tables: vec![
            "CREATE TABLE qn (id BIGINT PRIMARY KEY)",
            "CREATE TABLE qe (id BIGINT PRIMARY KEY, \"A\" BIGINT NOT NULL, \
             \"B\" BIGINT NOT NULL, \"Weight\" NUMERIC)",
            "INSERT INTO qn VALUES (1), (2), (3)",
            "INSERT INTO qe VALUES (1, 1, 2, 1), (2, 2, 3, 1)",
            extra_edge,
        ],
        indexes: vec![
            "CREATE INDEX qe_a ON qe (\"A\")",
            "CREATE INDEX qe_b ON qe (\"B\")",
        ],
        graph: "CREATE PROPERTY GRAPH qg VERTEX TABLES (qn AS n KEY (id) LABEL node \
                PROPERTIES (id)) EDGE TABLES (qe AS e KEY (id) SOURCE KEY (\"A\") \
                REFERENCES n (id) DESTINATION KEY (\"B\") REFERENCES n (id) LABEL link \
                PROPERTIES (\"Weight\"))",
        call: "'qg', cassie_graph.vertex_ref('qg', 'n', ARRAY[cassie_graph.key_part(1::bigint)]), \
               cassie_graph.vertex_ref('qg', 'n', ARRAY[cassie_graph.key_part(3::bigint)]), \
               'Weight', 2"
            .to_owned(),
        workers: 1,
    }
}

#[test]
fn should_answer_identically_for_quoted_mixed_case_columns_under_every_read_path() {
    // Arrange
    let case = quoted("INSERT INTO qe VALUES (3, 1, 9, -1)");

    // Act
    let all = answers(&case);

    // Assert
    let expected = ok(&[&["1", "2", "{1,1}"]]);
    assert!(all.iter().all(|found| *found == expected));
}

#[test]
fn should_reject_a_visible_invalid_weight_of_a_quoted_mixed_case_column_under_every_read_path() {
    // Arrange
    let case = quoted("INSERT INTO qe VALUES (3, 1, 3, -1)");

    // Act
    let all = answers(&case);

    // Assert
    assert!(all.iter().all(|found| *found == Err("22023".to_owned())));
}

fn graph_call(source: i64, target: i64) -> String {
    let vertex = |id: i64| {
        format!("cassie_graph.vertex_ref('ng', 'n', ARRAY[cassie_graph.key_part({id}::bigint)])")
    };
    format!(
        "SELECT p.path_rank::text, p.cost::text FROM graph_weighted_paths('ng', {}, {}, \
         'weight', 2) AS p ORDER BY p.path_rank",
        vertex(source),
        vertex(target)
    )
}

/// authored/not run: option (a) of N2. Dropping the original unique key succeeds only once a
/// second key exists; a REPEATABLE READ transaction opened before that sees neither the new
/// key (not in its snapshot) nor the dropped one, so validation reads the vertex keys in
/// full. The answer must equal the one the same transaction gave before the swap.
#[test]
fn should_answer_the_same_when_a_repeatable_read_cannot_see_the_replacing_key_index() {
    // Arrange
    let fixture = started(
        None,
        &[
            "CREATE TABLE pn (id BIGINT NOT NULL)",
            "CREATE UNIQUE INDEX pn_first ON pn (id)",
            "CREATE TABLE en (id BIGINT PRIMARY KEY, s BIGINT, d BIGINT, weight NUMERIC)",
            "INSERT INTO pn VALUES (1), (2), (3)",
            "INSERT INTO en VALUES (1, 1, 2, 1), (2, 2, 3, 1), (3, 1, 9, -1)",
            "CREATE PROPERTY GRAPH ng VERTEX TABLES (pn AS n KEY (id) LABEL node \
             PROPERTIES (id)) EDGE TABLES (en AS e KEY (id) SOURCE KEY (s) \
             REFERENCES n (id) DESTINATION KEY (d) REFERENCES n (id) LABEL link \
             PROPERTIES (weight))",
        ],
    );
    let writer = fixture.cassie.create_session("tester", None);
    let full_rows = || {
        fixture.cassie.metrics()["read_paths"]["graph_full_rows"]
            .as_u64()
            .expect("counter")
    };
    let sql = graph_call(1, 3);
    fixture.rows("BEGIN ISOLATION LEVEL REPEATABLE READ");
    let first = fixture.rows(&sql);
    let started_at = full_rows();
    let repeated = fixture.rows(&sql);
    let indexed_reads = full_rows() - started_at;

    // Act
    let created =
        fixture
            .cassie
            .execute_sql(&writer, "CREATE UNIQUE INDEX pn_second ON pn (id)", vec![]);
    let dropped = fixture
        .cassie
        .execute_sql(&writer, "DROP INDEX pn_first", vec![]);
    let before_swap = full_rows();
    let replaced = fixture.rows(&sql);
    let scanned_reads = full_rows() - before_swap;
    fixture.rows("ROLLBACK");

    // Assert
    assert!(created.is_ok());
    assert!(dropped.is_ok());
    assert_eq!(first, repeated);
    assert_eq!(first, replaced);
    assert!(scanned_reads > indexed_reads);
}
