//! `graph_weighted_paths` against the independent oracle across endpoints,
//! directions, hop limits, `k`, cost limits and label filters on one graph
//! with a zero-cost two-cycle, a self-loop, parallel edges and two labels.

use crate::graph_oracle::model::{Direction, SCALE};
use crate::world::{fast, slow, Link, OracleCall, Wg};

const DIRECTIONS: [(Direction, &str); 3] = [
    (Direction::Forward, "out"),
    (Direction::Backward, "in"),
    (Direction::Undirected, "both"),
];
const ENDPOINTS: [(i64, i64); 5] = [(1, 4), (1, 1), (2, 3), (4, 1), (3, 3)];

fn links() -> Vec<Link> {
    vec![
        fast(1, 1, 2, 1),
        fast(2, 2, 1, 0),
        fast(3, 1, 2, 3),
        slow(4, 2, 3, 2),
        fast(5, 3, 3, 1),
        fast(6, 3, 4, 0),
        slow(7, 1, 4, 6),
    ]
}

/// The oracle's rejection as the SQLSTATE the engine reports for the same call.
fn sql_state(rejection: &str) -> String {
    match rejection {
        "InvalidWeight" => "22023",
        "Overflow" => "22003",
        "UnknownVertex" => "42704",
        other => other,
    }
    .to_owned()
}

/// The index of a disagreeing combination with the SQLSTATE of each side, if it failed.
type Disagreement = (usize, Option<String>, Option<String>);

/// Compares every argument combination of `world` with the oracle, the whole result
/// including a failing SQLSTATE. Returns the number compared and the disagreements.
fn compare(world: &Wg) -> (usize, Vec<Disagreement>) {
    let mut compared = 0_usize;
    let mut mismatches = Vec::new();
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
                            let expected = world
                                .oracle(from, to, |_, _| OracleCall {
                                    direction,
                                    labels: only_fast.then(|| vec!["fast".to_owned()]),
                                    max_cost: max_cost.map(|cost| cost * SCALE),
                                    ..OracleCall::new(max_hops, k)
                                })
                                .map_err(|rejection| sql_state(&rejection));
                            compared += 1;
                            if actual != expected {
                                mismatches.push((compared, actual.err(), expected.err()));
                            }
                        }
                    }
                }
            }
        }
    }
    (compared, mismatches)
}

#[test]
pub fn should_agree_with_the_oracle_for_every_argument_combination() {
    // Arrange
    let world = Wg::new(4, &links());

    // Act
    let (compared, mismatches) = compare(&world);

    // Assert
    assert_eq!(mismatches, Vec::<Disagreement>::new());
    assert_eq!(compared, 360);
}

/// authored/not run: the same matrix with a negative weight on a slow edge, so the calls that
/// select only `fast` return rows while every other call fails with 22023, under every read
/// path.
#[test]
pub fn should_agree_with_the_oracle_when_an_invalid_weight_decides_the_answer() {
    // Arrange
    let mut all = links();
    all.push(slow(8, 2, 4, -1));
    let world = Wg::new(4, &all);

    // Act
    let (compared, mismatches) = compare(&world);

    // Assert
    assert_eq!(mismatches, Vec::<Disagreement>::new());
    assert_eq!(compared, 360);
}
