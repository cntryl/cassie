//! Two bounded path patterns joined on a shared typed vertex (#1353): exact multiplicity
//! against explicit relational joins, tenant-local composite identity, binding errors,
//! per-pattern metrics and EXPLAIN.

use crate::shared_entity_fixture::{cells, strings, unindexed_world, world};
use crate::support_sql_fixture::SqlFixture;
use cassie::types::Value;

const COMMON_NEIGHBORS: &str = "SELECT g.tenant, g.p, g.q, g.n FROM GRAPH_TABLE (social MATCH \
     (a IS person) -[IS knows]-> (n IS person), (b IS person) -[IS knows]-> (n) \
     WHERE a.id < b.id \
     COLUMNS (n.tenant AS tenant, a.id AS p, b.id AS q, n.id AS n)) AS g";

const FORK: &str = "SELECT g.tenant, g.r, g.x, g.y FROM GRAPH_TABLE (social MATCH \
     (r IS person) -[IS knows]-> (x IS person), (r) -[IS knows]-> (y IS person) \
     WHERE x.id < y.id \
     COLUMNS (r.tenant AS tenant, r.id AS r, x.id AS x, y.id AS y)) AS g";

const PERMISSION: &str = "SELECT g.rev_tenant, g.rev_id, g.doc_tenant, g.doc_id FROM GRAPH_TABLE \
     (social MATCH (req IS person WHERE req.tenant = $1 AND req.id = $2) \
     -[IS knows]->{1,2} (rev IS person), (rev) -[IS owns]-> (d IS document) \
     COLUMNS (rev.tenant AS rev_tenant, rev.id AS rev_id, d.tenant AS doc_tenant, \
     d.id AS doc_id)) AS g";

/// The common neighbors as two explicit relational joins over the mapped tables.
const COMMON_NEIGHBORS_JOIN: &str = "SELECT k1.dst_tenant, k1.src_id, k2.src_id, k1.dst_id \
     FROM knows k1 JOIN knows k2 ON k1.dst_tenant = k2.dst_tenant AND k1.dst_id = k2.dst_id \
     JOIN person s1 ON s1.tenant = k1.src_tenant AND s1.id = k1.src_id \
     JOIN person s2 ON s2.tenant = k2.src_tenant AND s2.id = k2.src_id \
     JOIN person n ON n.tenant = k1.dst_tenant AND n.id = k1.dst_id \
     WHERE k1.src_id < k2.src_id";

/// The same join on the printed id alone, which merges tenants.
const COMMON_NEIGHBORS_DISPLAY_JOIN: &str = "SELECT k1.dst_tenant, k1.src_id, k2.src_id, \
     k1.dst_id FROM knows k1 JOIN knows k2 ON k1.dst_id = k2.dst_id \
     WHERE k1.src_id < k2.src_id";

const FORK_JOIN: &str = "SELECT k1.src_tenant, k1.src_id, k1.dst_id, k2.dst_id \
     FROM knows k1 JOIN knows k2 ON k1.src_tenant = k2.src_tenant AND k1.src_id = k2.src_id \
     JOIN person r ON r.tenant = k1.src_tenant AND r.id = k1.src_id \
     JOIN person x ON x.tenant = k1.dst_tenant AND x.id = k1.dst_id \
     JOIN person y ON y.tenant = k2.dst_tenant AND y.id = k2.dst_id \
     WHERE k1.dst_id < k2.dst_id";

const PERMISSION_JOIN: &str = "SELECT k.dst_tenant, k.dst_id, o.doc_tenant, o.doc_id \
     FROM knows k JOIN owns o ON o.person_tenant = k.dst_tenant AND o.person_id = k.dst_id \
     JOIN document d ON d.tenant = o.doc_tenant AND d.id = o.doc_id \
     WHERE k.src_tenant = $1 AND k.src_id = $2 \
     UNION ALL \
     SELECT k2.dst_tenant, k2.dst_id, o.doc_tenant, o.doc_id \
     FROM knows k1 JOIN knows k2 ON k2.src_tenant = k1.dst_tenant AND k2.src_id = k1.dst_id \
     JOIN owns o ON o.person_tenant = k2.dst_tenant AND o.person_id = k2.dst_id \
     JOIN document d ON d.tenant = o.doc_tenant AND d.id = o.doc_id \
     WHERE k1.src_tenant = $1 AND k1.src_id = $2";

pub(crate) fn run(world: &SqlFixture, sql: &str, tenant: &str, id: i64) -> Vec<String> {
    let params = vec![Value::String(tenant.to_owned()), Value::Int64(id)];
    match world.cassie.execute_sql(&world.session, sql, params) {
        Ok(result) => cells(result.rows().to_vec()),
        Err(error) => panic!("statement failed with SQLSTATE {}", error.sql_state()),
    }
}

fn sorted(world: &SqlFixture, sql: &str) -> Vec<String> {
    cells(world.rows(sql))
}

fn state(world: &SqlFixture, sql: &str) -> String {
    world.error(sql).sql_state().to_owned()
}

fn pinned_common_neighbors() -> Vec<String> {
    strings(&["a|1|2|3", "a|1|2|3", "a|1|2|4", "a|3|4|5", "b|1|2|3"])
}

#[test]
fn should_match_two_relational_joins_for_a_converging_path() {
    // Arrange
    let world = world();

    // Act
    let native = sorted(&world, COMMON_NEIGHBORS);
    let relational = sorted(&world, COMMON_NEIGHBORS_JOIN);

    // Assert
    assert_eq!(native, relational);
    assert_eq!(native, pinned_common_neighbors());
}

#[test]
fn should_match_two_relational_joins_for_a_fork() {
    // Arrange
    let world = world();

    // Act
    let native = sorted(&world, FORK);
    let relational = sorted(&world, FORK_JOIN);

    // Assert
    assert_eq!(native, relational);
    assert_eq!(
        native,
        strings(&["a|1|3|4", "a|1|3|4", "a|2|3|4", "b|1|2|3"])
    );
}

#[test]
fn should_keep_the_multiplicity_of_parallel_edges_in_both_patterns() {
    // Arrange
    let world = world();
    let sql = "SELECT g.p FROM GRAPH_TABLE (social MATCH \
               (a IS person WHERE a.tenant = 'a' AND a.id = 1) -[IS knows]-> (n IS person), \
               (b IS person WHERE b.tenant = 'a' AND b.id = 1) -[IS knows]-> (n) \
               COLUMNS (n.id AS p)) AS g";

    // Act
    let rows = sorted(&world, sql);

    // Assert
    // Vertex 1 reaches 3 through two parallel edges and 4 through one: 2 * 2 + 1 * 1.
    assert_eq!(rows, strings(&["3", "3", "3", "3", "4"]));
}

#[test]
fn should_not_merge_tenants_that_print_the_same_key_components() {
    // Arrange
    let world = world();

    // Act
    let native = sorted(&world, COMMON_NEIGHBORS);
    let display = sorted(&world, COMMON_NEIGHBORS_DISPLAY_JOIN);

    // Assert
    assert_ne!(native, display);
    assert!(display.len() > native.len());
    assert!(native.iter().all(|row| {
        let parts = row.split('|').collect::<Vec<_>>();
        parts[0] == "a" || parts[0] == "b"
    }));
    assert_eq!(
        native.iter().filter(|row| row.starts_with("b|")).count(),
        1,
        "tenant b matches only its own common neighbor"
    );
}

#[test]
fn should_return_the_typed_identity_of_each_common_neighbor() {
    // Arrange
    let world = world();
    let sql = "SELECT g.tenant, g.n FROM GRAPH_TABLE (social MATCH \
               (a IS person) -[IS knows]-> (n IS person), (b IS person) -[IS knows]-> (n) \
               WHERE a.id = 1 AND b.id = 2 \
               COLUMNS (n.tenant AS tenant, n.id AS n)) AS g";

    // Act
    let rows = sorted(&world, sql);

    // Assert
    assert_eq!(rows, strings(&["a|3", "a|3", "a|4", "b|3"]));
}

#[test]
fn should_count_common_neighbors_per_pair_without_merging_tenants() {
    // Arrange
    let world = world();
    let sql = "SELECT g.tenant, g.p, g.q, COUNT(DISTINCT g.n) FROM GRAPH_TABLE (social MATCH \
               (a IS person) -[IS knows]-> (n IS person), (b IS person) -[IS knows]-> (n) \
               WHERE a.id < b.id \
               COLUMNS (n.tenant AS tenant, a.id AS p, b.id AS q, n.id AS n)) AS g \
               GROUP BY g.tenant, g.p, g.q";

    // Act
    let rows = sorted(&world, sql);

    // Assert
    assert_eq!(rows, strings(&["a|1|2|2", "a|3|4|1", "b|1|2|1"]));
}

#[test]
fn should_answer_the_permission_example_with_a_bounded_first_pattern() {
    // Arrange
    let world = world();

    // Act
    let native = run(&world, PERMISSION, "a", 1);
    let relational = run(&world, PERMISSION_JOIN, "a", 1);

    // Assert
    assert_eq!(native, relational);
    assert_eq!(
        native,
        strings(&["a|3|a|1", "a|3|a|1", "a|5|a|2", "a|5|a|2", "a|5|a|2"])
    );
}

#[test]
fn should_keep_each_tenant_inside_its_own_permission_result() {
    // Arrange
    let world = world();

    // Act
    let tenant_b = run(&world, PERMISSION, "b", 1);
    let relational = run(&world, PERMISSION_JOIN, "b", 1);
    let missing = run(&world, PERMISSION, "c", 1);

    // Assert
    assert_eq!(tenant_b, relational);
    assert_eq!(tenant_b, strings(&["b|2|b|2", "b|3|b|1", "b|3|b|1"]));
    assert_eq!(missing, Vec::<String>::new());
}

#[test]
fn should_return_no_rows_when_one_pattern_has_no_matches() {
    // Arrange
    let world = world();
    let empty_second = "SELECT g.a FROM GRAPH_TABLE (social MATCH \
         (a IS person) -[IS knows]-> (n IS person), \
         (n) -[IS owns]-> (d IS document WHERE d.title = 'none') COLUMNS (a.id AS a)) AS g";
    let empty_first = "SELECT g.a FROM GRAPH_TABLE (social MATCH \
         (a IS person WHERE a.id = 99) -[IS knows]-> (n IS person), \
         (n) -[IS owns]-> (d IS document) COLUMNS (a.id AS a)) AS g";

    // Act
    let second = sorted(&world, empty_second);
    let first = sorted(&world, empty_first);

    // Assert
    assert_eq!(second, Vec::<String>::new());
    assert_eq!(first, Vec::<String>::new());
}

#[test]
fn should_apply_a_selector_to_each_pattern_before_the_join() {
    // Arrange
    let world = world();
    let sql = "SELECT g.tenant, g.p, g.q, g.n FROM GRAPH_TABLE (social MATCH \
               (a IS person) -[IS knows]-> (n IS person), (b IS person) -[IS knows]-> (n) \
               KEEP ANY SHORTEST PATH WHERE a.id < b.id \
               COLUMNS (n.tenant AS tenant, a.id AS p, b.id AS q, n.id AS n)) AS g";

    // Act
    let rows = sorted(&world, sql);

    // Assert
    // One path per start and end pair collapses the parallel 1 -> 3 edges.
    assert_eq!(rows, strings(&["a|1|2|3", "a|1|2|4", "a|3|4|5", "b|1|2|3"]));
}

#[test]
fn should_return_the_same_rows_given_forced_exact_reads_and_unindexed_edges() {
    // Arrange
    let indexed = world();
    let plain = unindexed_world();
    let lazy = (
        sorted(&indexed, COMMON_NEIGHBORS),
        sorted(&indexed, FORK),
        run(&indexed, PERMISSION, "a", 1),
    );
    indexed.cassie.set_graph_exact_reads(true);

    // Act
    let exact = (
        sorted(&indexed, COMMON_NEIGHBORS),
        sorted(&indexed, FORK),
        run(&indexed, PERMISSION, "a", 1),
    );
    let fallback = (
        sorted(&plain, COMMON_NEIGHBORS),
        sorted(&plain, FORK),
        run(&plain, PERMISSION, "a", 1),
    );

    // Assert
    assert_eq!(lazy, exact);
    assert_eq!(lazy, fallback);
    assert_eq!(lazy.0, pinned_common_neighbors());
}

#[test]
fn should_order_joined_rows_by_each_patterns_path_order() {
    // Arrange
    let world = world();
    let sql = "SELECT g.p, g.q FROM GRAPH_TABLE (social MATCH \
               (a IS person WHERE a.tenant = 'a') -[IS knows]-> (n IS person), \
               (b IS person WHERE b.tenant = 'a') -[IS knows]-> (n) \
               WHERE a.id < b.id COLUMNS (a.id AS p, b.id AS q)) AS g";

    // Act
    let first = crate::shared_entity_fixture::ordered(world.rows(sql));
    let second = crate::shared_entity_fixture::ordered(world.rows(sql));

    // Assert
    assert_eq!(first, second);
    assert_eq!(first, strings(&["1|2", "1|2", "1|2", "3|4"]));
}

#[test]
fn should_read_shared_vertex_properties_from_every_pattern() {
    // Arrange
    let world = world();
    let sql = "SELECT g.who, g.title FROM GRAPH_TABLE (social MATCH \
               (a IS person WHERE a.tenant = 'a' AND a.id = 1) -[IS knows]-> (n IS person), \
               (n) -[IS owns]-> (d IS document) \
               COLUMNS (n.name AS who, d.title AS title)) AS g";

    // Act
    let rows = sorted(&world, sql);

    // Assert
    assert_eq!(rows, strings(&["cat|spec", "cat|spec"]));
}

#[test]
fn should_reject_unsupported_pattern_lists_with_the_documented_states() {
    // Arrange
    let world = world();
    let cases = [
        (
            "(a IS person) -[e IS knows]-> (b IS person), (b) -[e IS knows]-> (c IS person)",
            "0A000",
        ),
        (
            "(a IS person) -[IS knows]-> (b IS person), (c IS person) -[IS knows]-> (d IS person)",
            "0A000",
        ),
        (
            "(a IS person) -[IS knows]-> (b), (b) -[IS knows]-> (c), (c) -[IS knows]-> (d)",
            "0A000",
        ),
        (
            "p = (a IS person) -[IS knows]-> (b), (b) -[IS knows]-> (c)",
            "0A000",
        ),
        (
            "(a IS person) -[IS knows]-> (b), q = (b) -[IS knows]-> (c)",
            "0A000",
        ),
        (
            "(a IS person) -[IS knows]-> (n IS person), (b IS person WHERE b.id < a.id) \
             -[IS knows]-> (n)",
            "42703",
        ),
        (
            "(a IS person) -[IS owns]-> (n IS document), (n IS person) -[IS knows]-> (m)",
            "42804",
        ),
    ];

    // Act
    let states = cases
        .iter()
        .map(|(pattern, _)| {
            state(
                &world,
                &format!(
                    "SELECT * FROM GRAPH_TABLE (social MATCH {pattern} COLUMNS (a.id AS x)) AS g"
                ),
            )
        })
        .collect::<Vec<_>>();

    // Assert
    let expected = cases
        .iter()
        .map(|(_, state)| (*state).to_owned())
        .collect::<Vec<_>>();
    assert_eq!(states, expected);
}

#[test]
fn should_reject_an_unbounded_walk_in_a_later_pattern() {
    // Arrange
    let world = world();
    let sql = "SELECT * FROM GRAPH_TABLE (social MATCH \
               (a IS person) -[IS knows]-> (n IS person), (n) -[IS knows]->+ (m IS person) \
               COLUMNS (a.id AS x)) AS g";

    // Act
    let sqlstate = state(&world, sql);

    // Assert
    assert_eq!(sqlstate, "0A000");
}

#[test]
fn should_report_per_pattern_counts_in_the_metrics() {
    // Arrange
    let world = world();

    // Act
    let rows = run(&world, PERMISSION, "a", 1);
    let metrics = world.cassie.metrics();
    let read = |name: &str| {
        metrics["read_paths"][name]
            .as_array()
            .expect("per-pattern counters")
            .iter()
            .map(|count| count.as_u64().expect("count"))
            .collect::<Vec<_>>()
    };

    // Assert
    assert_eq!(rows.len(), 5);
    // The requester takes 3 first hops and 3 second hops; the reviewers {3, 4, 5} then take
    // their two ownership edges.
    assert_eq!(read("graph_last_branch_expansions"), vec![6, 2]);
    assert_eq!(read("graph_last_branch_paths"), vec![6, 2]);
}

#[test]
fn should_describe_the_join_in_explain() {
    // Arrange
    let world = world();

    // Act
    let rows = world.rows(&format!("EXPLAIN {COMMON_NEIGHBORS}"));
    let plan = match rows[0].first() {
        Some(Value::String(plan)) => plan.clone(),
        _ => panic!("EXPLAIN returns one text plan"),
    };

    // Assert
    assert!(plan.contains("patterns=2 join=shared_vertex"));
    assert!(plan.contains("pattern[0]{"));
    assert!(plan.contains("pattern[1]{"));
}
