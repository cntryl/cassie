//! The statement's own transaction view of `graph_weighted_paths`: uncommitted inserts,
//! updates and deletes of edges, weights and vertices decide the answer and the validation,
//! under the lazy indexed store with and without endpoint indexes and under the exact
//! full read (#1258, #1268).

use crate::graph_oracle::model::SCALE;
use crate::world::{fast, Variant, VariantGuard, Wg};

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

/// One world per read path over vertices `1..=3` and the single edge `1 -> 2` of weight 5.
fn worlds() -> Vec<Wg> {
    VARIANTS
        .into_iter()
        .map(|variant| {
            let _guard = VariantGuard::set(variant);
            Wg::new(3, &[fast(1, 1, 2, 5)])
        })
        .collect()
}

fn costs(world: &Wg, from: i64, to: i64) -> Vec<i128> {
    world
        .call(from, to, "'weight', 3")
        .expect("call succeeds")
        .iter()
        .map(|row| row.cost)
        .collect()
}

#[test]
fn should_see_an_own_inserted_edge() {
    for world in worlds() {
        // Arrange
        world.run("BEGIN");

        // Act
        world.run("INSERT INTO fast VALUES (2, 2, 3, 7, 'x', 1.5)");
        let inside = costs(&world, 1, 3);
        world.run("ROLLBACK");
        let after = costs(&world, 1, 3);

        // Assert
        assert_eq!(inside, vec![12 * SCALE]);
        assert_eq!(after, Vec::<i128>::new());
    }
}

#[test]
fn should_see_an_own_updated_weight() {
    for world in worlds() {
        // Arrange
        world.run("BEGIN");

        // Act
        world.run("UPDATE fast SET weight = 9 WHERE id = 1");
        let inside = costs(&world, 1, 2);
        world.run("ROLLBACK");
        let after = costs(&world, 1, 2);

        // Assert
        assert_eq!(inside, vec![9 * SCALE]);
        assert_eq!(after, vec![5 * SCALE]);
    }
}

#[test]
fn should_not_see_an_own_deleted_edge() {
    for world in worlds() {
        // Arrange
        world.run("BEGIN");

        // Act
        world.run("DELETE FROM fast WHERE id = 1");
        let inside = costs(&world, 1, 2);
        world.run("COMMIT");
        let after = costs(&world, 1, 2);

        // Assert
        assert_eq!(inside, Vec::<i128>::new());
        assert_eq!(after, Vec::<i128>::new());
    }
}

#[test]
fn should_reject_an_own_update_to_a_negative_weight_until_it_is_rolled_back() {
    for world in worlds() {
        // Arrange
        world.run("BEGIN");

        // Act
        world.run("UPDATE fast SET weight = -1 WHERE id = 1");
        let inside = world.call(1, 2, "'weight', 3");
        world.run("ROLLBACK");
        let after = costs(&world, 1, 2);

        // Assert
        assert_eq!(inside.err().as_deref(), Some("22023"));
        assert_eq!(after, vec![5 * SCALE]);
    }
}

#[test]
fn should_reject_an_own_inserted_edge_without_a_weight_between_existing_vertices() {
    for world in worlds() {
        // Arrange
        world.run("BEGIN");

        // Act
        world.run("INSERT INTO fast VALUES (2, 3, 1, NULL, 'x', 1.5)");
        let inside = world.call(1, 2, "'weight', 3");
        world.run("ROLLBACK");
        let after = costs(&world, 1, 2);

        // Assert
        assert_eq!(inside.err().as_deref(), Some("22023"));
        assert_eq!(after, vec![5 * SCALE]);
    }
}

#[test]
fn should_ignore_an_own_inserted_invalid_edge_whose_endpoint_does_not_exist_yet() {
    for world in worlds() {
        // Arrange
        world.run("BEGIN");
        world.run("INSERT INTO fast VALUES (2, 3, 4, NULL, 'x', 1.5)");

        // Act
        let before_vertex = world.call(1, 2, "'weight', 3");
        world.run("INSERT INTO v VALUES (4)");
        let with_vertex = world.call(1, 2, "'weight', 3");
        world.run("ROLLBACK");

        // Assert
        assert_eq!(
            before_vertex.expect("a dangling edge is not an edge").len(),
            1
        );
        assert_eq!(with_vertex.err().as_deref(), Some("22023"));
    }
}

#[test]
fn should_ignore_an_invalid_edge_whose_endpoint_vertex_was_deleted_in_the_transaction() {
    for world in worlds() {
        // Arrange
        world.run("INSERT INTO fast VALUES (2, 3, 2, NULL, 'x', 1.5)");
        let committed = world.call(1, 2, "'weight', 3");
        world.run("BEGIN");

        // Act
        world.run("DELETE FROM v WHERE id = 3");
        let inside = world.call(1, 2, "'weight', 3");
        world.run("ROLLBACK");

        // Assert
        assert_eq!(committed.err().as_deref(), Some("22023"));
        assert_eq!(inside.expect("the edge lost its source vertex").len(), 1);
    }
}
