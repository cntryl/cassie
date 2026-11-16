//! `graph_weighted_paths` against the independent oracle across endpoints,
//! directions, hop limits, `k`, cost limits and label filters on one graph
//! with a zero-cost two-cycle, a self-loop, parallel edges and two labels.

use crate::graph_oracle::model::{Direction, SCALE};
use crate::world::{fast, slow, OracleCall, Wg};

const DIRECTIONS: [(Direction, &str); 3] = [
    (Direction::Forward, "out"),
    (Direction::Backward, "in"),
    (Direction::Undirected, "both"),
];
const ENDPOINTS: [(i64, i64); 5] = [(1, 4), (1, 1), (2, 3), (4, 1), (3, 3)];

fn world() -> Wg {
    Wg::new(
        4,
        &[
            fast(1, 1, 2, 1),
            fast(2, 2, 1, 0),
            fast(3, 1, 2, 3),
            slow(4, 2, 3, 2),
            fast(5, 3, 3, 1),
            fast(6, 3, 4, 0),
            slow(7, 1, 4, 6),
        ],
    )
}

#[test]
fn should_agree_with_the_oracle_for_every_argument_combination() {
    // Arrange
    let world = world();
    let mut compared = 0_usize;
    let mut mismatches = Vec::new();

    // Act
    for (from, to) in ENDPOINTS {
        for (direction, direction_text) in DIRECTIONS {
            for max_hops in [0_u32, 1, 3] {
                for k in [1_u32, 3] {
                    for max_cost in [None, Some(3_i128)] {
                        for only_fast in [false, true] {
                            let labels = if only_fast { "ARRAY['fast']" } else { "NULL" };
                            let limit = max_cost.map_or("NULL".to_owned(), |cost| cost.to_string());
                            let rest = format!(
                                "'weight', {max_hops}, '{direction_text}', {labels}, {limit}, {k}"
                            );
                            let actual = world.call(from, to, &rest);
                            let expected = world.oracle(from, to, |_, _| OracleCall {
                                direction,
                                labels: only_fast.then(|| vec!["fast".to_owned()]),
                                max_cost: max_cost.map(|cost| cost * SCALE),
                                ..OracleCall::new(max_hops, k)
                            });
                            compared += 1;
                            let agree = match (&actual, &expected) {
                                (Ok(actual), Ok(expected)) => actual == expected,
                                (Err(_), Err(_)) => true,
                                _ => false,
                            };
                            if !agree {
                                mismatches.push(format!("{from}->{to}: {rest}"));
                            }
                        }
                    }
                }
            }
        }
    }

    // Assert
    assert_eq!(mismatches, Vec::<String>::new());
    assert_eq!(compared, 360);
}
