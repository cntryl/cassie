//! `graph_weighted_paths` against the independent oracle on the contract's required cases.

use crate::graph_oracle::model::{Direction, SCALE};
use crate::world::{fast, fast_micro, slow, unweighted, OracleCall, Wg};

#[test]
pub fn should_reject_a_hidden_negative_edge_even_when_the_cheapest_path_avoids_it() {
    // Arrange
    let world = Wg::new(
        3,
        &[fast(1, 1, 3, 5), fast(2, 1, 2, 10), fast(3, 2, 3, -100)],
    );

    // Act
    let call = world.call(1, 3, "'weight', 1");
    let expected = world.oracle(1, 3, |_, _| OracleCall::new(1, 1));

    // Assert
    assert_eq!(call.err().as_deref(), Some("22023"));
    assert!(expected.is_err());
}

#[test]
pub fn should_reject_a_disconnected_invalid_edge() {
    // Arrange
    let world = Wg::new(5, &[fast(1, 1, 2, 1), unweighted(2, 4, 5)]);

    // Act
    let call = world.call(1, 2, "'weight', 3");

    // Assert
    assert_eq!(call.err().as_deref(), Some("22023"));
}

#[test]
pub fn should_ignore_an_invalid_edge_that_the_label_filter_excludes() {
    // Arrange
    let world = Wg::new(
        3,
        &[fast(1, 1, 2, 4), slow(2, 2, 3, 1), unweighted(3, 3, 1)],
    );
    let call = OracleCall {
        labels: Some(vec!["slow".to_owned()]),
        ..OracleCall::new(3, 2)
    };

    // Act
    let rows = world.call(2, 3, "'weight', 3, 'out', ARRAY['slow'], NULL, 2");

    // Assert
    let expected = world.oracle(2, 3, |_, _| call).expect("oracle accepts");
    assert_eq!(rows.expect("call succeeds"), expected);
}

#[test]
pub fn should_reject_an_invalid_edge_of_a_selected_label() {
    // Arrange
    let world = Wg::new(
        3,
        &[fast(1, 1, 2, 4), slow(2, 2, 3, 1), unweighted(3, 3, 1)],
    );

    // Act
    let call = world.call(2, 3, "'weight', 3, 'out', ARRAY['fast']");

    // Assert
    assert_eq!(call.err().as_deref(), Some("22023"));
}

#[test]
pub fn should_select_no_edge_for_an_empty_label_array() {
    // Arrange
    let world = Wg::new(2, &[fast(1, 1, 2, 4)]);

    // Act
    let to_other = world.call(1, 2, "'weight', 3, 'out', ARRAY[]::text[]");
    let to_self = world.call(1, 1, "'weight', 3, 'out', ARRAY[]::text[]");

    // Assert
    assert_eq!(to_other.expect("call succeeds"), vec![]);
    assert_eq!(to_self.expect("call succeeds").len(), 1);
}

#[test]
pub fn should_return_the_zero_hop_path_before_the_closing_cycle_of_a_zero_cost_two_cycle() {
    // Arrange
    let world = Wg::new(2, &[fast(1, 1, 2, 0), fast(2, 2, 1, 0)]);

    // Act
    let rows = world
        .call(1, 1, "'weight', 2, 'out', NULL, NULL, 2")
        .expect("call succeeds");

    // Assert
    let expected = world
        .oracle(1, 1, |_, _| OracleCall::new(2, 2))
        .expect("oracle accepts");
    assert_eq!(rows, expected);
    assert_eq!(
        rows.iter()
            .map(|row| row.vertices.len())
            .collect::<Vec<_>>(),
        vec![1, 3]
    );
}

#[test]
pub fn should_order_equal_cost_paths_by_the_contract_path_order() {
    // Arrange
    let world = Wg::new(
        4,
        &[
            fast(1, 1, 2, 1),
            fast(2, 2, 4, 1),
            fast(3, 1, 3, 1),
            fast(4, 3, 4, 1),
        ],
    );

    // Act
    let rows = world
        .call(1, 4, "'weight', 2, 'out', NULL, NULL, 5")
        .expect("call succeeds");

    // Assert
    let expected = world
        .oracle(1, 4, |_, _| OracleCall::new(2, 5))
        .expect("oracle accepts");
    assert_eq!(rows, expected);
    assert_eq!(rows.len(), 2);
}

#[test]
pub fn should_order_parallel_edges_of_different_mappings_by_mapping_identity() {
    // Arrange
    let world = Wg::new(2, &[fast(1, 1, 2, 3), slow(1, 1, 2, 3), fast(2, 1, 2, 3)]);

    // Act
    let rows = world
        .call(1, 2, "'weight', 1, 'out', NULL, NULL, 3")
        .expect("call succeeds");

    // Assert
    let expected = world
        .oracle(1, 2, |_, _| OracleCall::new(1, 3))
        .expect("oracle accepts");
    assert_eq!(rows, expected);
    assert_eq!(rows.len(), 3);
}

#[test]
pub fn should_include_a_path_whose_cost_equals_max_cost() {
    // Arrange
    let world = Wg::new(3, &[fast(1, 1, 2, 2), fast(2, 2, 3, 3), fast(3, 1, 3, 6)]);
    let call = OracleCall {
        max_cost: Some(5 * SCALE),
        ..OracleCall::new(2, 5)
    };

    // Act
    let rows = world
        .call(1, 3, "'weight', 2, 'out', NULL, 5, 5")
        .expect("call succeeds");

    // Assert
    assert_eq!(
        rows,
        world.oracle(1, 3, |_, _| call).expect("oracle accepts")
    );
    assert_eq!(rows.len(), 1);
}

#[test]
pub fn should_exclude_paths_longer_than_max_hops() {
    // Arrange
    let world = Wg::new(3, &[fast(1, 1, 2, 1), fast(2, 2, 3, 1)]);

    // Act
    let short = world.call(1, 3, "'weight', 1").expect("call succeeds");
    let enough = world.call(1, 3, "'weight', 2").expect("call succeeds");

    // Assert
    assert_eq!((short.len(), enough.len()), (0, 1));
}

#[test]
pub fn should_follow_edges_backwards_for_direction_in() {
    // Arrange
    let world = Wg::new(3, &[fast(1, 2, 1, 4), fast(2, 3, 2, 5)]);
    let call = OracleCall {
        direction: Direction::Backward,
        ..OracleCall::new(2, 3)
    };

    // Act
    let rows = world
        .call(1, 3, "'weight', 2, 'in', NULL, NULL, 3")
        .expect("call succeeds");

    // Assert
    assert_eq!(
        rows,
        world.oracle(1, 3, |_, _| call).expect("oracle accepts")
    );
    assert_eq!(rows[0].directions, vec![2, 2]);
}

#[test]
pub fn should_follow_edges_both_ways_for_direction_both() {
    // Arrange
    let world = Wg::new(
        4,
        &[
            fast(1, 1, 2, 4),
            fast(2, 3, 2, 5),
            fast(3, 3, 3, 1),
            fast(4, 2, 4, 1),
        ],
    );
    let call = OracleCall {
        direction: Direction::Undirected,
        ..OracleCall::new(3, 6)
    };

    // Act
    let rows = world
        .call(1, 3, "'weight', 3, 'both', NULL, NULL, 6")
        .expect("call succeeds");

    // Assert
    assert_eq!(
        rows,
        world.oracle(1, 3, |_, _| call).expect("oracle accepts")
    );
    assert_ne!(rows.len(), 0);
}

#[test]
pub fn should_return_the_zero_hop_path_for_a_source_equal_to_its_target() {
    // Arrange
    let world = Wg::new(2, &[fast(1, 1, 2, 3)]);

    // Act
    let rows = world.call(1, 1, "'weight', 0").expect("call succeeds");

    // Assert
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (
            rows[0].cost,
            rows[0].vertices.len(),
            rows[0].edge_costs.len()
        ),
        (0, 1, 0)
    );
}

#[test]
pub fn should_return_a_self_loop_as_the_closing_path_of_its_vertex() {
    // Arrange
    let world = Wg::new(1, &[fast(1, 1, 1, 2)]);

    // Act
    let rows = world
        .call(1, 1, "'weight', 1, 'out', NULL, NULL, 2")
        .expect("call succeeds");

    // Assert
    let expected = world
        .oracle(1, 1, |_, _| OracleCall::new(1, 2))
        .expect("oracle accepts");
    assert_eq!(rows, expected);
    assert_eq!(rows.len(), 2);
}

#[test]
pub fn should_keep_fractional_weights_exact() {
    // Arrange
    let world = Wg::new(
        3,
        &[fast_micro(1, 1, 2, 100_000), fast_micro(2, 2, 3, 200_000)],
    );

    // Act
    let rows = world.call(1, 3, "'weight', 2").expect("call succeeds");

    // Assert
    assert_eq!(rows[0].cost, 300_000);
    assert_eq!(rows[0].edge_costs, vec![100_000, 200_000]);
}

#[test]
pub fn should_agree_with_the_oracle_on_a_dense_graph_for_every_k() {
    // Arrange
    let mut links = Vec::new();
    let mut id = 0;
    for from in 1..=4 {
        for to in 1..=4 {
            if from != to {
                id += 1;
                links.push(fast(id, from, to, (from * 3 + to) % 4));
            }
        }
    }
    let world = Wg::new(4, &links);

    // Act
    let outcomes = (1..=8).map(|k| {
        let rest = format!("'weight', 3, 'out', NULL, NULL, {k}");
        (
            world.call(1, 4, &rest),
            world.oracle(1, 4, |_, _| OracleCall::new(3, k)),
        )
    });

    // Assert
    for (actual, expected) in outcomes {
        assert_eq!(
            actual.expect("call succeeds"),
            expected.expect("oracle accepts")
        );
    }
}

#[test]
pub fn should_agree_with_the_oracle_when_an_in_range_cost_limit_cuts_the_answer() {
    // Arrange
    let world = Wg::new(
        4,
        &[
            fast(1, 1, 2, 1),
            fast(2, 2, 4, 1),
            fast(3, 1, 3, 2),
            fast(4, 3, 4, 2),
        ],
    );

    // Act
    let outcomes = (1..=5).map(|limit| {
        let rest = format!("'weight', 2, 'out', NULL, {limit}, 4");
        let call = OracleCall {
            max_cost: Some(i128::from(limit) * SCALE),
            ..OracleCall::new(2, 4)
        };
        (world.call(1, 4, &rest), world.oracle(1, 4, |_, _| call))
    });

    // Assert
    for (actual, expected) in outcomes {
        assert_eq!(
            actual.expect("call succeeds"),
            expected.expect("oracle accepts")
        );
    }
}

#[test]
pub fn should_fail_with_22003_when_a_cost_sum_is_unrepresentable() {
    // Arrange
    let world = Wg::new(3, &[fast(1, 1, 2, 1), fast(2, 2, 3, 1)]);
    world.run("UPDATE fast SET weight = 9 * (10::numeric ^ 131071)");

    // Act
    let call = world.call(1, 3, "'weight', 2");
    let single_hop = world.fixture.rows(&format!(
        "SELECT count(*) FROM graph_weighted_paths('wg', {}, {}, 'weight', 1) AS p",
        crate::world::vertex(1),
        crate::world::vertex(2)
    ));

    // Assert
    assert_eq!(call.err().as_deref(), Some("22003"));
    assert_eq!(single_hop, vec![vec![cassie::types::Value::Int64(1)]]);
}

#[test]
pub fn should_ignore_the_weight_of_an_edge_whose_endpoint_does_not_exist() {
    // Arrange
    let world = Wg::new(2, &[fast(1, 1, 2, 1), unweighted(2, 1, 99)]);

    // Act
    let rows = world.call(1, 2, "'weight', 2");

    // Assert
    let expected = world
        .oracle(1, 2, |_, _| OracleCall::new(2, 1))
        .expect("oracle accepts");
    assert_eq!(rows.expect("call succeeds"), expected);
}

#[test]
pub fn should_count_the_validation_scan_apart_from_traversal_reads() {
    // Arrange
    let world = Wg::new(3, &[fast(1, 1, 2, 1), fast(2, 2, 3, 1), fast(3, 1, 3, 5)]);
    let counter = |name: &str| {
        world.fixture.cassie.metrics()["read_paths"][name]
            .as_u64()
            .expect("counter")
    };
    let (validation, full) = (counter("graph_validation_rows"), counter("graph_full_rows"));

    // Act
    let rows = world.call(1, 3, "'weight', 2");

    // Assert
    assert!(rows.is_ok());
    assert_eq!(counter("graph_validation_rows") - validation, 3);
    // The edge mapping has no endpoint index here, so the search reads it in full once, apart
    // from the validation; vertices are fetched by their key.
    assert_eq!(counter("graph_full_rows") - full, 3);
}
