//! Unit tests of the oracle itself on the contract's required cases.

use super::enumerate::enumerate;
use super::model::{Direction, Edge, ElementRef, Graph, KeyPart, Path, Scalar, Vertex, SCALE};
use super::pattern::{
    EdgePattern, Mode, OracleError, Pattern, Quantifier, Selector, VertexPattern,
};
use super::select::match_paths;
use super::weighted::{weighted_paths, WeightedQuery};

const A: i64 = 1;
const B: i64 = 2;
const C: i64 = 3;
const D: i64 = 4;
const T: i64 = 5;

fn key(n: i64) -> Vec<KeyPart> {
    vec![KeyPart::BigInt(n)]
}

fn vertex(id: i64) -> Vertex {
    Vertex::new("v", 10, key(id), &[])
}

fn vref(id: i64) -> ElementRef {
    vertex(id).id
}

fn edge(id: i64, from: i64, to: i64) -> Edge {
    Edge::new("e", 20, key(id), &[], &vref(from), &vref(to))
}

fn graph(vertices: &[i64], edges: Vec<Edge>) -> Graph {
    Graph {
        vertices: vertices.iter().map(|id| vertex(*id)).collect(),
        edges,
    }
}

fn id_of(element: &ElementRef) -> i64 {
    match element.key[0] {
        KeyPart::BigInt(n) => n,
        _ => panic!("test keys are bigint"),
    }
}

fn vids(path: &Path) -> Vec<i64> {
    path.vertices.iter().map(id_of).collect()
}

fn eids(path: &Path) -> Vec<i64> {
    path.edges.iter().map(id_of).collect()
}

fn is_vertex(id: i64) -> VertexPattern {
    VertexPattern::any().when(move |v| id_of(&v.id) == id)
}

fn active(e: &Edge) -> bool {
    e.props.get("active") == Some(&Scalar::Bool(true))
}

fn forward(q: Quantifier) -> EdgePattern {
    EdgePattern::new(Direction::Forward).quantified(q)
}

fn weighted(id: i64, from: i64, to: i64, w: Scalar) -> Edge {
    edge(id, from, to).prop("w", w)
}

fn query<'a>(
    source: &'a ElementRef,
    target: &'a ElementRef,
    max_hops: u32,
    k: u32,
) -> WeightedQuery<'a> {
    WeightedQuery {
        source,
        target,
        weight_property: "w",
        max_hops,
        direction: Direction::Forward,
        edge_labels: None,
        max_cost: None,
        k,
    }
}

#[test]
fn should_differ_between_inline_versus_post_selection_predicates() {
    // Arrange
    let g = graph(
        &[A, B, C],
        vec![
            edge(1, A, B).prop("active", Scalar::Bool(false)),
            edge(2, A, C).prop("active", Scalar::Bool(true)),
            edge(3, C, B).prop("active", Scalar::Bool(true)),
        ],
    );
    let inline = Pattern::new(is_vertex(A))
        .step(
            forward(Quantifier::between(1, 2)).when(active),
            is_vertex(B),
        )
        .select(Selector::AnyShortest);
    let post = Pattern::new(is_vertex(A))
        .step(forward(Quantifier::between(1, 2)), is_vertex(B))
        .select(Selector::AnyShortest)
        .keep_where(|graph, path| {
            path.edges
                .iter()
                .all(|id| graph.edges.iter().any(|e| &e.id == id && active(e)))
        });
    // Act
    let inline_result = match_paths(&g, &inline).unwrap();
    let post_result = match_paths(&g, &post).unwrap();
    // Assert
    assert_eq!(
        inline_result.iter().map(vids).collect::<Vec<_>>(),
        vec![vec![A, C, B]]
    );
    assert!(post_result.is_empty());
}

#[test]
fn should_select_two_hop_path_with_bounds_two_to_three() {
    // Arrange
    let g = graph(
        &[A, B, C],
        vec![edge(1, A, B), edge(2, A, C), edge(3, C, B)],
    );
    let pattern = Pattern::new(is_vertex(A)).step(forward(Quantifier::between(2, 3)), is_vertex(B));
    // Act
    let result = match_paths(&g, &pattern).unwrap();
    // Assert
    assert_eq!(
        result.iter().map(vids).collect::<Vec<_>>(),
        vec![vec![A, C, B]]
    );
}

#[test]
fn should_bind_one_vertex_for_zero_hop_paths() {
    // Arrange
    let g = graph(&[A, B], vec![edge(1, A, B)]);
    let star =
        Pattern::new(is_vertex(A)).step(forward(Quantifier::between(0, 1)), VertexPattern::any());
    let both_labels = Pattern::new(VertexPattern::labeled(&["x"])).step(
        forward(Quantifier::between(0, 0)),
        VertexPattern::labeled(&["y"]),
    );
    let labeled = Graph {
        vertices: vec![Vertex::new("v", 10, key(A), &["x"])],
        edges: Vec::new(),
    };
    // Act
    let result = match_paths(&g, &star).unwrap();
    let mismatch = match_paths(&labeled, &both_labels).unwrap();
    // Assert
    assert_eq!(
        result.iter().map(vids).collect::<Vec<_>>(),
        vec![vec![A], vec![A, B]]
    );
    assert!(result[0].edges.is_empty() && result[0].directions.is_empty());
    assert!(mismatch.is_empty());
}

#[test]
fn should_distinguish_simple_from_acyclic_on_two_cycle() {
    // Arrange
    let g = graph(&[A, B], vec![edge(1, A, B), edge(2, B, A)]);
    let pattern = |mode| {
        Pattern::new(is_vertex(A))
            .step(forward(Quantifier::exactly(2)), is_vertex(A))
            .mode(mode)
    };
    // Act
    let simple = match_paths(&g, &pattern(Mode::Simple)).unwrap();
    let acyclic = match_paths(&g, &pattern(Mode::Acyclic)).unwrap();
    // Assert
    assert_eq!(
        simple.iter().map(vids).collect::<Vec<_>>(),
        vec![vec![A, B, A]]
    );
    assert!(acyclic.is_empty());
}

#[test]
fn should_treat_undirected_self_loop_as_one_step() {
    // Arrange
    let g = graph(&[A], vec![edge(1, A, A)]);
    let pattern = Pattern::new(is_vertex(A)).step(
        EdgePattern::new(Direction::Undirected),
        VertexPattern::any(),
    );
    // Act
    let result = match_paths(&g, &pattern).unwrap();
    // Assert
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].directions, vec![1]);
}

#[test]
fn should_record_reverse_direction_for_undirected_non_loop_edge() {
    // Arrange
    let g = graph(&[A, B], vec![edge(1, A, B)]);
    let pattern = |start| {
        Pattern::new(is_vertex(start)).step(
            EdgePattern::new(Direction::Undirected),
            VertexPattern::any(),
        )
    };
    // Act
    let from_a = match_paths(&g, &pattern(A)).unwrap();
    let from_b = match_paths(&g, &pattern(B)).unwrap();
    // Assert
    assert_eq!(from_a[0].directions, vec![1]);
    assert_eq!(from_b[0].directions, vec![2]);
}

#[test]
fn should_keep_parallel_edges_as_distinct_paths() {
    // Arrange
    let g = graph(&[A, B], vec![edge(1, A, B), edge(2, A, B)]);
    let pattern = Pattern::new(is_vertex(A)).step(forward(Quantifier::one()), is_vertex(B));
    // Act
    let result = match_paths(&g, &pattern).unwrap();
    // Assert
    assert_eq!(
        result.iter().map(eids).collect::<Vec<_>>(),
        vec![vec![1], vec![2]]
    );
}

#[test]
fn should_match_label_alternatives_once() {
    // Arrange
    let g = Graph {
        vertices: vec![vertex(A), Vertex::new("v", 10, key(B), &["x", "y"])],
        edges: vec![edge(1, A, B)],
    };
    let pattern = Pattern::new(is_vertex(A)).step(
        forward(Quantifier::one()),
        VertexPattern::labeled(&["x", "y"]),
    );
    // Act
    let result = match_paths(&g, &pattern).unwrap();
    // Assert
    assert_eq!(result.len(), 1);
}

#[test]
fn should_break_equal_length_tie_by_path_order() {
    // Arrange: z-edges (90, 91) run through B; a-edges (10, 11) through C.
    let g = graph(
        &[A, B, C, D],
        vec![
            edge(90, A, B),
            edge(91, B, D),
            edge(10, A, C),
            edge(11, C, D),
        ],
    );
    let pattern = Pattern::new(is_vertex(A))
        .step(forward(Quantifier::between(1, 3)), is_vertex(D))
        .select(Selector::AnyShortest);
    // Act
    let result = match_paths(&g, &pattern).unwrap();
    // Assert
    assert_eq!(
        result.iter().map(vids).collect::<Vec<_>>(),
        vec![vec![A, C, D]]
    );
}

#[test]
fn should_select_shortest_k_per_endpoint_pair() {
    // Arrange
    let g = graph(
        &[A, B, C, D],
        vec![
            edge(1, A, D),
            edge(2, A, B),
            edge(3, B, D),
            edge(4, A, C),
            edge(5, C, D),
        ],
    );
    let pattern = Pattern::new(is_vertex(A))
        .step(forward(Quantifier::between(1, 2)), is_vertex(D))
        .select(Selector::Shortest(2));
    // Act
    let result = match_paths(&g, &pattern).unwrap();
    // Assert
    assert_eq!(
        result.iter().map(vids).collect::<Vec<_>>(),
        vec![vec![A, D], vec![A, B, D]]
    );
}

#[test]
fn should_exclude_edges_with_absent_endpoint() {
    // Arrange
    let g = graph(&[A, B], vec![edge(1, A, 99), edge(2, A, B)]);
    let pattern = Pattern::new(is_vertex(A)).step(forward(Quantifier::one()), VertexPattern::any());
    // Act
    let result = match_paths(&g, &pattern).unwrap();
    // Assert
    assert_eq!(result.iter().map(eids).collect::<Vec<_>>(), vec![vec![2]]);
    assert_eq!(g.live_edges().count(), 1);
}

#[test]
fn should_order_zero_hop_path_before_closing_cycle() {
    // Arrange
    let g = graph(&[A, B], vec![edge(1, A, B), edge(2, B, A)]);
    let pattern = Pattern::new(is_vertex(A))
        .step(forward(Quantifier::star()), is_vertex(A))
        .mode(Mode::Simple);
    // Act
    let result = match_paths(&g, &pattern).unwrap();
    // Assert
    assert_eq!(
        result.iter().map(vids).collect::<Vec<_>>(),
        vec![vec![A], vec![A, B, A]]
    );
}

#[test]
fn should_reject_unbounded_walk_but_allow_unbounded_trail() {
    // Arrange
    let g = graph(&[A, B], vec![edge(1, A, B), edge(2, B, A)]);
    let walk = Pattern::new(is_vertex(A)).step(forward(Quantifier::plus()), is_vertex(T));
    let trail = Pattern::new(is_vertex(A))
        .step(forward(Quantifier::plus()), VertexPattern::any())
        .mode(Mode::Trail);
    // Act
    let walk_result = enumerate(&g, &walk);
    let trail_result = enumerate(&g, &trail).unwrap();
    // Assert
    assert_eq!(walk_result.unwrap_err(), OracleError::UnboundedWalk);
    assert_eq!(trail_result.len(), 2);
}

#[test]
fn should_reject_inverted_bounds_or_zero_k() {
    // Arrange
    let g = graph(&[A], Vec::new());
    let bad_bounds =
        Pattern::new(is_vertex(A)).step(forward(Quantifier::between(3, 2)), VertexPattern::any());
    let zero_k = Pattern::new(is_vertex(A)).select(Selector::Shortest(0));
    // Act
    let bounds = enumerate(&g, &bad_bounds).unwrap_err();
    let k = enumerate(&g, &zero_k).unwrap_err();
    // Assert
    assert_eq!(bounds, OracleError::InvalidBounds);
    assert_eq!(k, OracleError::ZeroK);
}

#[test]
fn should_fail_whole_call_for_hidden_negative_weight() {
    // Arrange
    let g = graph(
        &[A, B, T],
        vec![
            weighted(1, A, T, Scalar::Int(5)),
            weighted(2, A, B, Scalar::Int(10)),
            weighted(3, B, T, Scalar::Int(-100)),
        ],
    );
    let (source, target) = (vref(A), vref(T));
    // Act
    let result = weighted_paths(&g, &query(&source, &target, 1, 1));
    // Assert
    assert_eq!(result.unwrap_err(), OracleError::InvalidWeight);
}

#[test]
fn should_reject_null_missing_or_non_finite_weights() {
    // Arrange
    let (source, target) = (vref(A), vref(B));
    let make = |w: Option<Scalar>| {
        let mut e = edge(1, A, B);
        if let Some(w) = w {
            e = e.prop("w", w);
        }
        graph(&[A, B], vec![e])
    };
    let cases = [
        None,
        Some(Scalar::Null),
        Some(Scalar::NonFinite),
        Some(Scalar::Text("x".to_string())),
    ];
    // Act
    let results: Vec<_> = cases
        .into_iter()
        .map(|w| weighted_paths(&make(w), &query(&source, &target, 1, 1)))
        .collect();
    // Assert
    assert!(results
        .iter()
        .all(|r| r == &Err(OracleError::InvalidWeight)));
}

#[test]
fn should_ignore_weights_of_dangling_edges() {
    // Arrange
    let g = graph(
        &[A, B],
        vec![weighted(1, A, B, Scalar::Int(1)), edge(2, A, 99)],
    );
    let (source, target) = (vref(A), vref(B));
    // Act
    let result = weighted_paths(&g, &query(&source, &target, 1, 1)).unwrap();
    // Assert
    assert_eq!(result[0].cost, SCALE);
}

#[test]
fn should_return_zero_hop_then_closing_cycle_for_zero_cost_two_cycle() {
    // Arrange
    let g = graph(
        &[A, B],
        vec![
            weighted(1, A, B, Scalar::Int(0)),
            weighted(2, B, A, Scalar::Int(0)),
        ],
    );
    let a = vref(A);
    // Act
    let result = weighted_paths(&g, &query(&a, &a, 2, 2)).unwrap();
    // Assert
    assert_eq!(
        result.iter().map(|r| vids(&r.path)).collect::<Vec<_>>(),
        vec![vec![A], vec![A, B, A]]
    );
    assert!(result[0].edge_costs.is_empty());
    assert_eq!(result[1].edge_costs, vec![0, 0]);
    assert_eq!(
        result.iter().map(|r| r.rank).collect::<Vec<_>>(),
        vec![1, 2]
    );
}

#[test]
fn should_rank_cheapest_paths_by_tie_order_within_cost_limit() {
    // Arrange: A->T costs 4, A->B->T costs 4, A->C->T costs 4.5.
    let g = graph(
        &[A, B, C, T],
        vec![
            weighted(1, A, T, Scalar::Int(4)),
            weighted(2, A, B, Scalar::Int(1)),
            weighted(3, B, T, Scalar::Int(3)),
            weighted(4, A, C, Scalar::Int(2)),
            weighted(5, C, T, Scalar::Num(2 * SCALE + 500_000)),
        ],
    );
    let (source, target) = (vref(A), vref(T));
    let mut limited = query(&source, &target, 2, 5);
    limited.max_cost = Some(4 * SCALE);
    // Act
    let top_two = weighted_paths(&g, &query(&source, &target, 2, 2)).unwrap();
    let within = weighted_paths(&g, &limited).unwrap();
    // Assert
    assert_eq!(
        top_two.iter().map(|r| vids(&r.path)).collect::<Vec<_>>(),
        vec![vec![A, T], vec![A, B, T]]
    );
    assert_eq!(within.len(), 2);
}
