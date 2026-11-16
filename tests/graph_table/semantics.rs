//! Path-pattern semantics: the executor against the independent oracle.

use crate::graph_oracle::model::{Direction, Path, Scalar, Vertex};
use crate::graph_oracle::pattern::{
    EdgePattern, Mode, Pattern, Quantifier, Selector, VertexPattern,
};
use crate::world::{
    ends, link, node, vertices, vertices_and_edges, Keys, Link, Node, Storage, World,
};

fn nodes(count: i64) -> Vec<Node> {
    (1..=count).map(node).collect()
}

fn any_node() -> VertexPattern {
    VertexPattern::labeled(&["node"])
}

fn node_number(number: i64) -> VertexPattern {
    any_node().when(move |vertex| vertex.props.get("num") == Some(&Scalar::Int(number)))
}

fn forward(quantifier: Quantifier) -> EdgePattern {
    EdgePattern::new(Direction::Forward)
        .labeled(&["link"])
        .quantified(quantifier)
}

fn high_score(vertex: &Vertex) -> bool {
    matches!(vertex.props.get("score"), Some(Scalar::Int(score)) if *score > 2)
}

fn inactive(number: i64, from: i64, to: i64) -> Link {
    Link {
        active: false,
        ..link(number, from, to)
    }
}

fn first_middle_last(world: &World, path: &Path) -> Vec<i64> {
    let numbers = vertices(world, path);
    vec![numbers[0], numbers[1], numbers[numbers.len() - 1]]
}

#[test]
fn should_match_oracle_for_single_hop_edges() {
    // Arrange
    let world = World::simple(&nodes(3), &[link(1, 1, 2), link(2, 1, 3), link(3, 2, 3)]);
    let oracle = Pattern::new(any_node()).step(forward(Quantifier::one()), any_node());

    // Act
    let actual = world.query(
        "(s IS node) -[e IS link]-> (t IS node)",
        "s.num AS s, t.num AS t, e.num AS e",
    );

    // Assert
    assert_eq!(actual, world.oracle(&oracle, vertices_and_edges));
}

#[test]
fn should_return_every_vertex_for_a_vertex_only_pattern() {
    // Arrange
    let world = World::simple(&nodes(4), &[]);
    let oracle = Pattern::new(any_node());

    // Act
    let actual = world.query("(s IS node)", "s.num AS s");

    // Assert
    assert_eq!(actual, world.oracle(&oracle, vertices));
}

#[test]
fn should_honour_inclusive_bounds_of_two_to_three_hops() {
    // Arrange
    let world = World::simple(
        &nodes(4),
        &[link(1, 1, 2), link(2, 1, 3), link(3, 3, 2), link(4, 2, 4)],
    );
    let oracle = Pattern::new(any_node()).step(forward(Quantifier::between(2, 3)), any_node());

    // Act
    let agrees = world.query(
        "(s IS node) -[IS link]->{2,3} (t IS node)",
        "s.num AS s, t.num AS t",
    );

    // Assert
    assert_eq!(agrees, world.oracle(&oracle, ends));
}

#[test]
fn should_bind_both_endpoints_of_a_zero_hop_path_to_one_vertex() {
    // Arrange
    let world = World::simple(&nodes(3), &[link(1, 1, 2)]);
    let oracle = Pattern::new(any_node()).step(forward(Quantifier::between(0, 1)), any_node());

    // Act
    let actual = world.query(
        "(s IS node) -[IS link]->{0,1} (t IS node)",
        "s.num AS s, t.num AS t",
    );

    // Assert
    assert_eq!(actual, world.oracle(&oracle, ends));
}

#[test]
fn should_require_both_endpoint_predicates_on_a_zero_hop_vertex() {
    // Arrange
    let world = World::simple(&nodes(3), &[link(1, 1, 2)]);
    let oracle =
        Pattern::new(node_number(1)).step(forward(Quantifier::between(0, 1)), node_number(2));

    // Act
    let actual = world.query(
        "(s IS node WHERE s.num = 1) -[IS link]->{0,1} (t IS node WHERE t.num = 2)",
        "s.num AS s, t.num AS t",
    );

    // Assert
    assert_eq!(actual, world.oracle(&oracle, ends));
    assert_eq!(actual, vec![vec![1, 2]]);
}

#[test]
fn should_apply_inline_predicates_before_selection_unlike_the_graph_where() {
    // Arrange
    let world = World::simple(
        &nodes(5),
        &[
            link(1, 1, 2),
            link(2, 2, 4),
            link(3, 1, 3),
            link(4, 3, 5),
            link(5, 5, 4),
        ],
    );
    let inline = Pattern::new(node_number(1))
        .step(forward(Quantifier::one()), any_node().when(high_score))
        .step(forward(Quantifier::between(1, 3)), any_node())
        .select(Selector::AnyShortest);
    let post = Pattern::new(node_number(1))
        .step(forward(Quantifier::one()), any_node())
        .step(forward(Quantifier::between(1, 3)), any_node())
        .select(Selector::AnyShortest)
        .keep_where(|graph, path| graph.vertex(&path.vertices[1]).is_some_and(high_score));

    // Act
    let inline_rows = world.query(
        "(s IS node WHERE s.num = 1) -[IS link]-> (m IS node WHERE m.score > 2) \
         -[IS link]->{1,3} (t IS node) KEEP ANY SHORTEST PATH",
        "s.num AS s, m.num AS m, t.num AS t",
    );
    let post_rows = world.query(
        "(s IS node WHERE s.num = 1) -[IS link]-> (m IS node) -[IS link]->{1,3} (t IS node) \
         KEEP ANY SHORTEST PATH WHERE m.score > 2",
        "s.num AS s, m.num AS m, t.num AS t",
    );

    // Assert
    assert_eq!(inline_rows, world.oracle(&inline, first_middle_last));
    assert_eq!(post_rows, world.oracle(&post, first_middle_last));
    assert_ne!(inline_rows, post_rows);
}

#[test]
fn should_distinguish_simple_from_acyclic_on_a_two_cycle() {
    // Arrange
    let world = World::simple(&nodes(2), &[link(1, 1, 2), link(2, 2, 1)]);
    let simple = Pattern::new(any_node())
        .step(forward(Quantifier::between(1, 3)), any_node())
        .mode(Mode::Simple);
    let acyclic = Pattern::new(any_node())
        .step(forward(Quantifier::between(1, 3)), any_node())
        .mode(Mode::Acyclic);

    // Act
    let simple_rows = world.query(
        "(s IS node) -[IS link]->{1,3} (t IS node) KEEP ALL SIMPLE PATHS",
        "s.num AS s, t.num AS t",
    );
    let acyclic_rows = world.query(
        "(s IS node) -[IS link]->{1,3} (t IS node) KEEP ALL ACYCLIC PATHS",
        "s.num AS s, t.num AS t",
    );

    // Assert
    assert_eq!(simple_rows, world.oracle(&simple, ends));
    assert_eq!(acyclic_rows, world.oracle(&acyclic, ends));
    assert_eq!(simple_rows.len(), 4);
    assert_eq!(acyclic_rows.len(), 2);
}

#[test]
fn should_enumerate_bounded_walks_like_the_oracle() {
    // Arrange
    let world = World::simple(&nodes(3), &[link(1, 1, 2), link(2, 2, 1), link(3, 2, 3)]);
    let walk = Pattern::new(any_node()).step(forward(Quantifier::between(1, 4)), any_node());

    // Act
    let rows = world.query(
        "(s IS node) -[IS link]->{1,4} (t IS node)",
        "s.num AS s, t.num AS t",
    );

    // Assert
    assert_eq!(rows, world.oracle(&walk, ends));
}

#[test]
fn should_enumerate_unbounded_trails_like_the_oracle() {
    // Arrange
    let world = World::simple(&nodes(3), &[link(1, 1, 2), link(2, 2, 1), link(3, 2, 3)]);
    let trail = Pattern::new(any_node())
        .step(forward(Quantifier::star()), any_node())
        .mode(Mode::Trail);

    // Act
    let rows = world.query(
        "(s IS node) -[IS link]->* (t IS node) KEEP ALL TRAIL PATHS",
        "s.num AS s, t.num AS t",
    );

    // Assert
    assert_eq!(rows, world.oracle(&trail, ends));
}

#[test]
fn should_traverse_an_undirected_self_loop_once() {
    // Arrange
    let world = World::simple(&nodes(2), &[link(1, 1, 1), link(2, 1, 2)]);
    let oracle = Pattern::new(any_node()).step(
        EdgePattern::new(Direction::Undirected).labeled(&["link"]),
        any_node(),
    );

    // Act
    let actual = world.query(
        "(s IS node) -[e IS link]- (t IS node)",
        "s.num AS s, t.num AS t, e.num AS e",
    );

    // Assert
    assert_eq!(actual, world.oracle(&oracle, vertices_and_edges));
    assert_eq!(actual.iter().filter(|row| row[2] == 1).count(), 1);
}

#[test]
fn should_follow_edges_against_their_direction_like_the_oracle() {
    // Arrange
    let world = World::simple(&nodes(3), &[link(1, 1, 2), link(2, 3, 2)]);
    let incoming = Pattern::new(any_node()).step(
        EdgePattern::new(Direction::Backward).labeled(&["link"]),
        any_node(),
    );

    // Act
    let rows = world.query(
        "(s IS node) <-[e IS link]- (t IS node)",
        "s.num AS s, t.num AS t, e.num AS e",
    );

    // Assert
    assert_eq!(rows, world.oracle(&incoming, vertices_and_edges));
}

#[test]
fn should_follow_undirected_edges_in_both_orientations_like_the_oracle() {
    // Arrange
    let world = World::simple(&nodes(3), &[link(1, 1, 2), link(2, 3, 2)]);
    let undirected = Pattern::new(any_node()).step(
        EdgePattern::new(Direction::Undirected).labeled(&["link"]),
        any_node(),
    );

    // Act
    let rows = world.query(
        "(s IS node) -[e IS link]- (t IS node)",
        "s.num AS s, t.num AS t, e.num AS e",
    );

    // Assert
    assert_eq!(rows, world.oracle(&undirected, vertices_and_edges));
    assert_eq!(rows.len(), 4);
}

#[test]
fn should_keep_parallel_edges_as_distinct_matches() {
    // Arrange
    let world = World::simple(&nodes(2), &[link(1, 1, 2), link(2, 1, 2)]);
    let oracle = Pattern::new(any_node()).step(forward(Quantifier::one()), any_node());

    // Act
    let actual = world.query(
        "(s IS node) -[e IS link]-> (t IS node)",
        "s.num AS s, t.num AS t, e.num AS e",
    );

    // Assert
    assert_eq!(actual, world.oracle(&oracle, vertices_and_edges));
    assert_eq!(actual.len(), 2);
}

#[test]
fn should_choose_the_path_through_the_lower_edge_on_a_shortest_tie() {
    // Arrange
    let world = World::simple(
        &nodes(4),
        &[link(1, 1, 3), link(2, 3, 4), link(3, 1, 2), link(4, 2, 4)],
    );
    let oracle = Pattern::new(node_number(1))
        .step(forward(Quantifier::one()), any_node())
        .step(forward(Quantifier::one()), node_number(4))
        .select(Selector::AnyShortest);

    // Act
    let actual = world.query(
        "(s IS node WHERE s.num = 1) -[IS link]-> (m IS node) -[IS link]-> \
         (t IS node WHERE t.num = 4) KEEP ANY SHORTEST PATH",
        "s.num AS s, m.num AS m, t.num AS t",
    );

    // Assert
    assert_eq!(actual, world.oracle(&oracle, first_middle_last));
    assert_eq!(actual, vec![vec![1, 3, 4]]);
}

#[test]
fn should_return_the_k_lowest_tied_paths_per_endpoint_pair() {
    // Arrange
    let world = World::simple(
        &nodes(5),
        &[
            link(1, 1, 5),
            link(2, 1, 2),
            link(3, 2, 5),
            link(4, 1, 3),
            link(5, 3, 5),
            link(6, 1, 4),
            link(7, 4, 5),
        ],
    );
    let oracle = Pattern::new(node_number(1))
        .step(forward(Quantifier::between(1, 2)), node_number(5))
        .select(Selector::Shortest(3));

    // Act
    let actual = world.query(
        "(s IS node WHERE s.num = 1) -[IS link]->{1,2} (t IS node WHERE t.num = 5) \
         KEEP SHORTEST 3 PATHS",
        "s.num AS s, t.num AS t",
    );

    // Assert
    assert_eq!(actual, world.oracle(&oracle, ends));
    assert_eq!(actual.len(), 3);
}

#[test]
fn should_apply_an_inline_edge_predicate_to_each_group_edge() {
    // Arrange
    let world = World::simple(
        &nodes(3),
        &[inactive(1, 1, 3), link(2, 1, 2), link(3, 2, 3)],
    );
    let oracle = Pattern::new(node_number(1))
        .step(
            forward(Quantifier::between(1, 2))
                .when(|edge| edge.props.get("active") == Some(&Scalar::Bool(true))),
            node_number(3),
        )
        .select(Selector::AnyShortest);

    // Act
    let actual = world.query(
        "(s IS node WHERE s.num = 1) -[e IS link WHERE e.active]->{1,2} \
         (t IS node WHERE t.num = 3) KEEP ANY SHORTEST PATH",
        "s.num AS s, t.num AS t",
    );

    // Assert
    assert_eq!(actual, world.oracle(&oracle, ends));
    assert_eq!(actual, vec![vec![1, 3]]);
}

#[test]
fn should_bind_a_repeated_variable_to_one_vertex() {
    // Arrange
    let world = World::simple(&nodes(3), &[link(1, 1, 2), link(2, 2, 1), link(3, 2, 3)]);
    let oracle = Pattern::new(any_node())
        .step(forward(Quantifier::one()), any_node())
        .step(forward(Quantifier::one()), any_node());

    // Act
    let actual = world.query(
        "(a IS node) -[IS link]-> (b IS node) -[IS link]-> (a IS node)",
        "a.num AS a, b.num AS b",
    );

    // Assert
    let cycles = world
        .oracle(&oracle, vertices)
        .into_iter()
        .filter(|row| row[0] == row[2])
        .map(|row| vec![row[0], row[1]])
        .collect::<Vec<_>>();
    assert_eq!(actual, cycles);
}

#[test]
fn should_reject_a_predicate_that_is_not_true() {
    // Arrange
    let world = World::simple(&nodes(3), &[link(1, 1, 2)]);

    // Act
    let rows = world.query("(s IS node WHERE s.num > NULL)", "s.num AS s");

    // Assert
    assert_eq!(rows.len(), 0);
}

#[test]
fn should_agree_with_the_oracle_across_key_shapes() {
    // Arrange
    let links = [link(1, 1, 2), link(2, 2, 3), link(3, 1, 3), link(4, 3, 1)];
    let oracle = Pattern::new(any_node())
        .step(forward(Quantifier::between(1, 3)), any_node())
        .mode(Mode::Trail);

    // Act
    let results = [Keys::BigInt, Keys::Uuid, Keys::Composite].map(|keys| {
        let world = World::new(keys, Storage::Row, &nodes(3), &links);
        (
            world.query(
                "(s IS node) -[IS link]->{1,3} (t IS node) KEEP ALL TRAIL PATHS",
                "s.num AS s, t.num AS t",
            ),
            world.oracle(&oracle, ends),
        )
    });

    // Assert
    for (actual, expected) in results {
        assert_ne!(expected.len(), 0);
        assert_eq!(actual, expected);
    }
}

#[test]
fn should_agree_with_the_oracle_on_columnar_tables() {
    // Arrange
    let links = [link(1, 1, 2), link(2, 2, 3), link(3, 3, 1)];
    let oracle = Pattern::new(any_node())
        .step(forward(Quantifier::between(1, 3)), any_node())
        .mode(Mode::Simple);

    // Act
    let results = [Keys::BigInt, Keys::Uuid, Keys::Composite].map(|keys| {
        let world = World::new(keys, Storage::Columnar, &nodes(3), &links);
        (
            world.query(
                "(s IS node) -[IS link]->{1,3} (t IS node) KEEP ALL SIMPLE PATHS",
                "s.num AS s, t.num AS t",
            ),
            world.oracle(&oracle, ends),
        )
    });

    // Assert
    for (actual, expected) in results {
        assert_ne!(expected.len(), 0);
        assert_eq!(actual, expected);
    }
}

#[test]
fn should_count_a_path_once_across_adjacent_quantified_steps() {
    // Arrange
    let world = World::simple(&nodes(4), &[link(1, 1, 2), link(2, 2, 3), link(3, 3, 4)]);
    let oracle = Pattern::new(any_node())
        .step(forward(Quantifier::between(0, 2)), any_node())
        .step(forward(Quantifier::between(0, 2)), any_node());

    // Act
    let actual = world.query(
        "(s IS node) -[IS link]->{0,2} (m IS node) -[IS link]->{0,2} (t IS node)",
        "s.num AS s, t.num AS t",
    );

    // Assert
    assert_eq!(actual, world.oracle(&oracle, ends));
}

#[test]
fn should_evaluate_an_edge_predicate_that_reads_another_variable() {
    // Arrange
    let world = World::simple(&nodes(3), &[link(1, 1, 2), link(2, 1, 3), link(3, 2, 3)]);
    let oracle = Pattern::new(any_node()).step(forward(Quantifier::one()), any_node());

    // Act
    let actual = world.query(
        "(s IS node) -[e IS link WHERE e.num > s.num]-> (t IS node)",
        "s.num AS s, t.num AS t, e.num AS e",
    );

    // Assert
    let expected = world
        .oracle(&oracle, vertices_and_edges)
        .into_iter()
        .filter(|row| row[2] > row[0])
        .collect::<Vec<_>>();
    assert_eq!(actual, expected);
    assert_eq!(actual.len(), 2);
}
