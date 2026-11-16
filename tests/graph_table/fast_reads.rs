//! Lazy indexed graph reads: counted reads bounded by the visited neighborhood, the
//! statement's own writes and snapshot (#1253, #1254), reuse across `LATERAL` rows (#1259)
//! and the access paths `EXPLAIN` reports. Reads are counted, never timed.

use crate::world::{ints, link, node, Link, Node, Variant, VariantGuard, World};
use cassie::app::CassieSession;
use cassie::types::Value;

const INDEXED: Variant = Variant {
    exact: false,
    endpoint_indexes: true,
};
const PLAIN: Variant = Variant {
    exact: false,
    endpoint_indexes: false,
};

const OUTWARD_FROM_ONE: &str = "SELECT * FROM GRAPH_TABLE (g MATCH \
     (a IS node WHERE a.num = 1) -[IS link]-> (b IS node) COLUMNS (b.num AS b)) AS t \
     ORDER BY b";
const TWO_HOPS_FROM_ONE: &str = "SELECT * FROM GRAPH_TABLE (g MATCH \
     (a IS node WHERE a.num = 1) -[IS link]->{1,2} (b IS node) COLUMNS (b.num AS b)) AS t \
     ORDER BY b";
const EDGES: &str = "SELECT * FROM GRAPH_TABLE (g MATCH (s IS node) -[e IS link]-> (t IS node) \
                     COLUMNS (e.num AS e)) AS t ORDER BY e";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Counts {
    index_entries: u64,
    row_fetches: u64,
    full_rows: u64,
    overlay_changes: u64,
    cache_hits: u64,
    validation_rows: u64,
}

impl Counts {
    fn minus(self, before: Self) -> Self {
        Self {
            index_entries: self.index_entries - before.index_entries,
            row_fetches: self.row_fetches - before.row_fetches,
            full_rows: self.full_rows - before.full_rows,
            overlay_changes: self.overlay_changes - before.overlay_changes,
            cache_hits: self.cache_hits - before.cache_hits,
            validation_rows: self.validation_rows - before.validation_rows,
        }
    }

    fn reads(self) -> u64 {
        self.index_entries + self.row_fetches + self.full_rows
    }
}

fn counts(world: &World) -> Counts {
    let metrics = world.fixture.cassie.metrics();
    let read = |name: &str| metrics["read_paths"][name].as_u64().expect("counter");
    Counts {
        index_entries: read("graph_index_entries"),
        row_fetches: read("graph_row_fetches"),
        full_rows: read("graph_full_rows"),
        overlay_changes: read("graph_overlay_changes"),
        cache_hits: read("graph_cache_hits"),
        validation_rows: read("graph_validation_rows"),
    }
}

fn run(world: &World, session: &CassieSession, sql: &str) -> Vec<Vec<i64>> {
    match world.fixture.cassie.execute_sql(session, sql, vec![]) {
        Ok(result) => ints(result.rows().to_vec()),
        Err(error) => panic!("statement failed with SQLSTATE {}", error.sql_state()),
    }
}

fn exec(world: &World, session: &CassieSession, sql: &str) {
    if let Err(error) = world.fixture.cassie.execute_sql(session, sql, vec![]) {
        panic!("statement failed with SQLSTATE {}", error.sql_state());
    }
}

/// Rows and counted reads of one statement.
fn measure(world: &World, session: &CassieSession, sql: &str) -> (Vec<Vec<i64>>, Counts) {
    let before = counts(world);
    let rows = run(world, session, sql);
    (rows, counts(world).minus(before))
}

/// The statement's rows under the lazy store and under the forced exact read, on the same
/// data and in the same session.
fn both(world: &World, session: &CassieSession, sql: &str) -> (Vec<Vec<i64>>, Vec<Vec<i64>>) {
    world.fixture.cassie.set_graph_exact_reads(false);
    let lazy = run(world, session, sql);
    world.fixture.cassie.set_graph_exact_reads(true);
    let exact = run(world, session, sql);
    world.fixture.cassie.set_graph_exact_reads(false);
    (lazy, exact)
}

/// Vertex 1 with `degree` outgoing neighbors, plus `extra` unrelated edges in a ring of
/// vertices that no pattern from vertex 1 reaches.
fn star(degree: i64, extra: i64) -> (Vec<Node>, Vec<Link>) {
    let ring = 1000;
    let mut nodes = (1..=degree + 1).map(node).collect::<Vec<_>>();
    let mut links = (1..=degree).map(|n| link(n, 1, n + 1)).collect::<Vec<_>>();
    for step in 0..extra {
        nodes.push(node(ring + step));
        links.push(link(
            degree + 1 + step,
            ring + step,
            ring + (step + 1) % extra,
        ));
    }
    (nodes, links)
}

fn star_world(variant: Variant, degree: i64, extra: i64) -> World {
    let _guard = VariantGuard::set(variant);
    let (nodes, links) = star(degree, extra);
    World::simple(&nodes, &links)
}

#[test]
fn should_read_only_the_neighborhood_given_an_endpoint_index_when_the_graph_grows() {
    // Arrange
    let small = star_world(INDEXED, 3, 5);
    let large = star_world(INDEXED, 3, 90);

    // Act
    let (small_rows, small_reads) = measure(&small, &small.fixture.session, OUTWARD_FROM_ONE);
    let (large_rows, large_reads) = measure(&large, &large.fixture.session, OUTWARD_FROM_ONE);

    // Assert
    assert_eq!(small_rows, vec![vec![2], vec![3], vec![4]]);
    assert_eq!(small_rows, large_rows);
    assert_eq!(small_reads, large_reads);
    assert_eq!(small_reads.full_rows, 0);
    // One start vertex, three incident edges and the three far vertices.
    assert_eq!(small_reads.index_entries, 1 + 3 + 3);
    assert_eq!(small_reads.row_fetches, 1 + 3 + 3);
}

#[test]
fn should_read_the_neighborhoods_of_visited_vertices_given_two_hops_when_the_graph_grows() {
    // Arrange
    let small = star_world(INDEXED, 2, 5);
    let large = star_world(INDEXED, 2, 90);
    for world in [&small, &large] {
        exec(
            world,
            &world.fixture.session,
            "INSERT INTO node VALUES (50, 'n', 50)",
        );
        exec(
            world,
            &world.fixture.session,
            "INSERT INTO link VALUES (500, 2, 50, TRUE, 1), (501, 3, 50, TRUE, 1)",
        );
    }

    // Act
    let (small_rows, small_reads) = measure(&small, &small.fixture.session, TWO_HOPS_FROM_ONE);
    let (large_rows, large_reads) = measure(&large, &large.fixture.session, TWO_HOPS_FROM_ONE);

    // Assert
    assert_eq!(small_rows, vec![vec![2], vec![3], vec![50], vec![50]]);
    assert_eq!(small_rows, large_rows);
    assert_eq!(small_reads, large_reads);
    assert_eq!(small_reads.full_rows, 0);
    // The start; the edges of vertices 1, 2 and 3 (2 + 1 + 1); and the far vertices 2, 3 and
    // 50, which is fetched once although two edges reach it.
    assert_eq!(small_reads.index_entries, 1 + (2 + 1 + 1) + 3);
}

#[test]
fn should_read_every_edge_given_no_endpoint_index_when_the_graph_grows() {
    // Arrange
    let small = star_world(PLAIN, 3, 5);
    let large = star_world(PLAIN, 3, 90);

    // Act
    let (small_rows, small_reads) = measure(&small, &small.fixture.session, OUTWARD_FROM_ONE);
    let (large_rows, large_reads) = measure(&large, &large.fixture.session, OUTWARD_FROM_ONE);

    // Assert
    assert_eq!(small_rows, large_rows);
    assert_eq!(small_reads.full_rows, 3 + 5);
    assert_eq!(large_reads.full_rows, 3 + 90);
    assert!(large_reads.reads() > small_reads.reads());
}

#[test]
fn should_return_the_same_rows_given_anchored_and_unanchored_starts_under_both_reads() {
    // Arrange
    let world = star_world(INDEXED, 3, 6);
    let session = &world.fixture.session;
    let queries = [
        OUTWARD_FROM_ONE,
        TWO_HOPS_FROM_ONE,
        EDGES,
        "SELECT * FROM GRAPH_TABLE (g MATCH (a IS node WHERE a.num = 1 AND a.score = 1) \
         -[IS link]-> (b IS node) COLUMNS (b.num AS b)) AS t ORDER BY b",
        "SELECT * FROM GRAPH_TABLE (g MATCH (a IS node WHERE a.num = 1 AND a.score = 2) \
         -[IS link]-> (b IS node) COLUMNS (b.num AS b)) AS t ORDER BY b",
        "SELECT * FROM GRAPH_TABLE (g MATCH (a IS node WHERE a.num = 1 OR a.num = 2) \
         -[IS link]-> (b IS node) COLUMNS (b.num AS b)) AS t ORDER BY b",
        "SELECT * FROM GRAPH_TABLE (g MATCH (a IS node WHERE a.num = 99) \
         -[IS link]-> (b IS node) COLUMNS (b.num AS b)) AS t ORDER BY b",
        "SELECT * FROM GRAPH_TABLE (g MATCH (a IS node WHERE a.num = 1) <-[IS link]- \
         (b IS node) COLUMNS (b.num AS b)) AS t ORDER BY b",
        "SELECT * FROM GRAPH_TABLE (g MATCH (a IS node WHERE a.num = 2) -[IS link]- \
         (b IS node) COLUMNS (b.num AS b)) AS t ORDER BY b",
        "SELECT * FROM GRAPH_TABLE (g MATCH (a IS node WHERE a.num = 1) \
         -[IS link]->{0,3} (b IS node) KEEP ANY SHORTEST PATH COLUMNS (b.num AS b)) AS t \
         ORDER BY b",
    ];

    // Act
    let results = queries.map(|sql| both(&world, session, sql));

    // Assert
    for (lazy, exact) in results {
        assert_eq!(lazy, exact);
    }
}

#[test]
fn should_return_the_same_rows_given_a_parameter_start_under_both_reads() {
    // Arrange
    let world = star_world(INDEXED, 3, 6);
    let session = &world.fixture.session;
    let sql = "SELECT * FROM GRAPH_TABLE (g MATCH (a IS node WHERE a.num = $1) \
               -[IS link]-> (b IS node) COLUMNS (b.num AS b)) AS t ORDER BY b";
    let values = [Value::Int64(1), Value::Int64(1000), Value::Null];
    let mut observed = Vec::new();

    // Act
    for value in values {
        let mut rows = Vec::new();
        for exact in [false, true] {
            world.fixture.cassie.set_graph_exact_reads(exact);
            let result = world
                .fixture
                .cassie
                .execute_sql(session, sql, vec![value.clone()]);
            rows.push(match result {
                Ok(result) => ints(result.rows().to_vec()),
                Err(error) => panic!("statement failed with SQLSTATE {}", error.sql_state()),
            });
        }
        observed.push(rows);
    }

    // Assert
    for rows in observed {
        assert_eq!(rows[0], rows[1]);
    }
}

#[test]
fn should_use_a_point_lookup_for_a_parameter_start_given_a_key_equality() {
    // Arrange
    let world = star_world(INDEXED, 3, 90);
    let sql = "SELECT * FROM GRAPH_TABLE (g MATCH (a IS node WHERE a.num = $1) \
               -[IS link]-> (b IS node) COLUMNS (b.num AS b)) AS t";
    let before = counts(&world);

    // Act
    let rows = world
        .fixture
        .cassie
        .execute_sql(&world.fixture.session, sql, vec![Value::Int64(1)])
        .expect("statement")
        .rows()
        .len();
    let reads = counts(&world).minus(before);

    // Assert
    assert_eq!(rows, 3);
    assert_eq!(reads.full_rows, 0);
}

#[test]
fn should_merge_pending_edge_writes_with_a_bounded_read_given_an_open_transaction() {
    // Arrange
    let world = star_world(INDEXED, 2, 40);
    let session = &world.fixture.session;
    let (_, committed) = measure(&world, session, OUTWARD_FROM_ONE);
    exec(&world, session, "BEGIN");
    exec(
        &world,
        session,
        "INSERT INTO link VALUES (900, 1, 4, TRUE, 1)",
    );
    exec(&world, session, "UPDATE link SET dst = 5 WHERE num = 1");
    exec(&world, session, "DELETE FROM link WHERE num = 2");
    exec(
        &world,
        session,
        "INSERT INTO node VALUES (4, 'n', 4), (5, 'n', 5)",
    );

    // Act
    let (rows, pending) = measure(&world, session, OUTWARD_FROM_ONE);
    let (lazy, exact) = both(&world, session, OUTWARD_FROM_ONE);

    // Assert
    assert_eq!(rows, vec![vec![4], vec![5]]);
    assert_eq!((lazy, exact), (rows.clone(), rows));
    assert_eq!(pending.full_rows, 0);
    assert!(pending.overlay_changes >= 2);
    // The committed neighborhood plus the vertices the pending edges reach.
    assert!(pending.index_entries <= committed.index_entries + 2);
}

#[test]
fn should_keep_a_savepoint_rollback_exact_given_pending_edge_writes() {
    // Arrange
    let world = star_world(INDEXED, 2, 6);
    let session = &world.fixture.session;
    exec(&world, session, "BEGIN");
    exec(&world, session, "SAVEPOINT before_edge");
    exec(
        &world,
        session,
        "INSERT INTO link VALUES (900, 1, 4, TRUE, 1)",
    );
    exec(&world, session, "INSERT INTO node VALUES (4, 'n', 4)");

    // Act
    let inside = both(&world, session, OUTWARD_FROM_ONE);
    exec(&world, session, "ROLLBACK TO SAVEPOINT before_edge");
    let (rolled_back, reads) = measure(&world, session, OUTWARD_FROM_ONE);

    // Assert
    assert_eq!(inside.0, vec![vec![2], vec![3], vec![4]]);
    assert_eq!(inside.0, inside.1);
    assert_eq!(rolled_back, vec![vec![2], vec![3]]);
    assert_eq!(reads.overlay_changes, 0);
}

#[test]
fn should_hide_pending_edges_from_another_session_given_an_endpoint_index() {
    // Arrange
    let world = star_world(INDEXED, 2, 6);
    let writer = &world.fixture.session;
    let reader = world.fixture.cassie.create_session("tester", None);
    exec(&world, writer, "BEGIN");
    exec(&world, writer, "INSERT INTO node VALUES (4, 'n', 4)");
    exec(
        &world,
        writer,
        "INSERT INTO link VALUES (900, 1, 4, TRUE, 1)",
    );

    // Act
    let (before, reads) = measure(&world, &reader, OUTWARD_FROM_ONE);
    exec(&world, writer, "COMMIT");
    let after = run(&world, &reader, OUTWARD_FROM_ONE);

    // Assert
    assert_eq!(
        (before, after),
        (vec![vec![2], vec![3]], vec![vec![2], vec![3], vec![4]])
    );
    assert_eq!(reads.overlay_changes, 0);
}

#[test]
fn should_keep_a_repeatable_read_view_given_a_concurrent_writer_and_an_endpoint_index() {
    // Arrange
    let world = star_world(INDEXED, 2, 6);
    let reader = &world.fixture.session;
    let writer = world.fixture.cassie.create_session("tester", None);
    exec(&world, reader, "BEGIN ISOLATION LEVEL REPEATABLE READ");
    let first = run(&world, reader, OUTWARD_FROM_ONE);

    // Act
    exec(&world, &writer, "INSERT INTO node VALUES (4, 'n', 4)");
    exec(
        &world,
        &writer,
        "INSERT INTO link VALUES (900, 1, 4, TRUE, 1)",
    );
    exec(&world, &writer, "DELETE FROM link WHERE num = 1");
    let snapshot = both(&world, reader, OUTWARD_FROM_ONE);
    exec(&world, reader, "COMMIT");
    let fresh = run(&world, reader, OUTWARD_FROM_ONE);

    // Assert
    assert_eq!(first, vec![vec![2], vec![3]]);
    assert_eq!(snapshot.0, first);
    assert_eq!(snapshot.1, first);
    assert_eq!(fresh, vec![vec![3], vec![4]]);
}

const LATERAL_NEIGHBORS: &str = "SELECT o.k, t.b FROM generate_series(1, 6) AS o(k), \
     LATERAL GRAPH_TABLE (g MATCH (a IS node WHERE a.num = 1) -[IS link]-> (b IS node) \
     COLUMNS (b.num AS b)) AS t ORDER BY o.k, t.b";
const LATERAL_CORRELATED: &str = "SELECT o.k, t.b FROM generate_series(1, 3) AS o(k), \
     LATERAL GRAPH_TABLE (g MATCH (a IS node WHERE a.num = 1) -[IS link]->{1,2} \
     (b IS node WHERE b.num > o.k) COLUMNS (b.num AS b)) AS t ORDER BY o.k, t.b";

#[test]
fn should_reuse_neighborhoods_across_lateral_rows_given_a_shared_start() {
    // Arrange
    let world = star_world(INDEXED, 3, 40);
    let session = &world.fixture.session;
    let single = "SELECT * FROM GRAPH_TABLE (g MATCH (a IS node WHERE a.num = 1) \
                  -[IS link]-> (b IS node) COLUMNS (b.num AS b)) AS t ORDER BY b";
    let (_, once) = measure(&world, session, single);

    // Act
    let (rows, lateral) = measure(&world, session, LATERAL_NEIGHBORS);

    // Assert
    assert_eq!(rows.len(), 6 * 3);
    assert_eq!(lateral.index_entries, once.index_entries);
    assert_eq!(lateral.row_fetches, once.row_fetches);
    assert!(lateral.cache_hits >= 5);
    assert!(lateral.reads() < 6 * once.reads());
}

#[test]
fn should_return_the_rows_of_independent_traversals_given_lateral_reuse() {
    // Arrange
    let world = star_world(INDEXED, 3, 12);
    let session = &world.fixture.session;
    exec(&world, session, "INSERT INTO node VALUES (60, 'n', 60)");
    exec(
        &world,
        session,
        "INSERT INTO link VALUES (700, 2, 60, TRUE, 1)",
    );

    // Act
    let lazy = run(&world, session, LATERAL_CORRELATED);
    world.fixture.cassie.set_graph_exact_reads(true);
    let exact = run(&world, session, LATERAL_CORRELATED);
    world.fixture.cassie.set_graph_exact_reads(false);

    // Assert
    assert_eq!(lazy, exact);
    assert!(!lazy.is_empty());
}

fn explain(world: &World, sql: &str) -> String {
    let rows = world.fixture.rows(sql);
    match rows[0].first() {
        Some(Value::String(plan)) => plan.clone(),
        _ => panic!("EXPLAIN returns one text plan"),
    }
}

#[test]
fn should_name_the_indexed_access_paths_given_an_explain_with_endpoint_indexes() {
    // Arrange
    let world = star_world(INDEXED, 2, 4);

    // Act
    let plan = explain(&world, &format!("EXPLAIN {OUTWARD_FROM_ONE}"));

    // Assert
    assert!(plan.contains("graph_table="));
    assert!(plan.contains("vertex[n]=point_lookup(index="));
    assert!(plan.contains("start=anchored"));
    assert!(plan.contains("edge[l]=indexed(index=out:link_src)"));
    assert!(!plan.contains("graph_reads="));
}

#[test]
fn should_name_the_fallback_reason_given_an_explain_without_endpoint_indexes() {
    // Arrange
    let world = star_world(PLAIN, 2, 4);

    // Act
    let plan = explain(&world, &format!("EXPLAIN {OUTWARD_FROM_ONE}"));
    let forced = {
        world.fixture.cassie.set_graph_exact_reads(true);
        explain(&world, &format!("EXPLAIN {OUTWARD_FROM_ONE}"))
    };

    // Assert
    assert!(plan.contains("edge[l]=exact(reason=no_endpoint_index)"));
    assert!(forced.contains("vertex[n]=exact(reason=exact_reads_forced)"));
    assert!(forced.contains("edge[l]=exact(reason=exact_reads_forced)"));
}

#[test]
fn should_report_counted_reads_and_the_frontier_given_an_explain_analyze() {
    // Arrange
    let world = star_world(INDEXED, 3, 20);

    // Act
    let plan = explain(&world, &format!("EXPLAIN ANALYZE {OUTWARD_FROM_ONE}"));

    // Assert
    assert!(plan.contains("edge[l]=indexed("));
    assert!(plan.contains(
        "graph_reads=index_entries:7,row_fetches:7,full_rows:0,overlay_changes:0,cache_hits:0,\
         validation_rows:0,frontier_peak:1"
    ));
}

#[test]
fn should_report_cache_reuse_given_an_explain_analyze_of_a_lateral_source() {
    // Arrange
    let world = star_world(INDEXED, 3, 20);

    // Act
    let plan = explain(&world, &format!("EXPLAIN ANALYZE {LATERAL_NEIGHBORS}"));

    // Assert
    assert!(plan.contains("graph_reads=index_entries:7,row_fetches:7,full_rows:0"));
    assert!(!plan.contains("cache_hits:0,"));
}

const ANCHORED_WALKS: &str = "SELECT * FROM GRAPH_TABLE (g MATCH \
     (s IS node WHERE s.num = 1) -[IS link]->{1,2} (t IS node) \
     COLUMNS (s.num AS s, t.num AS t)) AS t";

fn limited_indexed(count: i64, budget: usize) -> World {
    let _guard = VariantGuard::set(INDEXED);
    crate::resources::limited(count, budget)
}

#[test]
fn should_enforce_the_measured_peak_as_the_budget_boundary_given_indexed_reads() {
    // Arrange
    let measured = limited_indexed(6, 64 * 1024 * 1024);
    let measured_rows = measured.fixture.rows(ANCHORED_WALKS).len();
    let needed = crate::resources::peak(&measured);

    // Act
    let below = crate::resources::failure(&limited_indexed(6, needed - 1), ANCHORED_WALKS);
    let at = limited_indexed(6, needed)
        .fixture
        .rows(ANCHORED_WALKS)
        .len();

    // Assert
    assert_eq!(below.sql_state(), "54000");
    assert_eq!((measured_rows, at), (30, 30));
}

#[test]
fn should_cancel_an_indexed_traversal_at_an_inline_predicate_boundary() {
    // Arrange
    let (nodes, links) = crate::resources::complete(4);
    let world = {
        let _guard = VariantGuard::set(INDEXED);
        World::simple(&nodes, &links)
    };
    let sql = "SELECT * FROM GRAPH_TABLE (g MATCH (s IS node WHERE s.num = 1) \
               -[IS link WHERE pg_sleep(0) IS NOT NULL]->{1,3} (t IS node) \
               COLUMNS (s.num AS s, t.num AS t)) AS t";
    let (_guard, selected, resume) = cassie::runtime::pause_next_query_at_sleep_entry();
    let cancellation = cassie::runtime::QueryCancellationHandle::new();

    // Act
    let outcome = std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            world.fixture.cassie.execute_sql_with_cancellation(
                &world.fixture.session,
                sql,
                vec![],
                &cancellation,
            )
        });
        selected
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("traversal reaches its first predicate");
        cancellation.cancel();
        resume.send(()).expect("resume the traversal");
        worker.join().expect("worker finishes")
    });

    // Assert
    match outcome {
        Ok(_) => panic!("a canceled traversal returns no rows"),
        Err(error) => assert_eq!(error.sql_state(), "57014"),
    }
}

#[test]
fn should_not_reuse_cached_reads_given_a_later_statement() {
    // Arrange
    let world = star_world(INDEXED, 2, 4);
    let session = &world.fixture.session;
    let (first, _) = measure(&world, session, OUTWARD_FROM_ONE);
    exec(&world, session, "INSERT INTO node VALUES (9, 'n', 9)");
    exec(
        &world,
        session,
        "INSERT INTO link VALUES (900, 1, 9, TRUE, 1)",
    );

    // Act
    let (second, reads) = measure(&world, session, OUTWARD_FROM_ONE);

    // Assert
    assert_eq!(first, vec![vec![2], vec![3]]);
    assert_eq!(second, vec![vec![2], vec![3], vec![9]]);
    assert_eq!(reads.cache_hits, 0);
}
