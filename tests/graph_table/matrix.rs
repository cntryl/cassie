//! The executor against the independent oracle across every mode, direction,
//! quantifier and selector on one graph with a two-cycle, a self-loop and
//! parallel edges.

use crate::graph_oracle::model::Direction;
use crate::graph_oracle::pattern::{
    EdgePattern, Mode, Pattern, Quantifier, Selector, VertexPattern,
};
use crate::world::{ends, link, node, World};

const MODES: [(Mode, &str); 4] = [
    (Mode::Walk, "WALK"),
    (Mode::Trail, "TRAIL"),
    (Mode::Acyclic, "ACYCLIC"),
    (Mode::Simple, "SIMPLE"),
];

/// A direction with the text before and after the bracketed edge.
const DIRECTIONS: [(Direction, &str, &str); 3] = [
    (Direction::Forward, "-", "->"),
    (Direction::Backward, "<-", "-"),
    (Direction::Undirected, "-", "-"),
];

fn bounds() -> [(Quantifier, &'static str); 5] {
    [
        (Quantifier::one(), ""),
        (Quantifier::exactly(2), "{2}"),
        (Quantifier::between(0, 2), "{0,2}"),
        (Quantifier::between(1, 3), "{1,3}"),
        (Quantifier::between(2, 3), "{2,3}"),
    ]
}

fn selectors(mode: &str) -> [(Selector, String); 3] {
    [
        (Selector::All, format!("KEEP ALL {mode} PATHS")),
        (
            Selector::AnyShortest,
            format!("KEEP ANY SHORTEST {mode} PATH"),
        ),
        (
            Selector::Shortest(2),
            format!("KEEP SHORTEST 2 {mode} PATHS"),
        ),
    ]
}

fn world() -> World {
    World::simple(
        &(1..=4).map(node).collect::<Vec<_>>(),
        &[
            link(1, 1, 2),
            link(2, 2, 1),
            link(3, 1, 2),
            link(4, 2, 3),
            link(5, 3, 3),
            link(6, 3, 4),
        ],
    )
}

#[test]
pub(crate) fn should_agree_with_the_oracle_for_every_mode_direction_bound_selector() {
    // Arrange
    let world = world();
    let mut compared = 0_usize;
    let mut mismatches = Vec::new();

    // Act
    for (mode, mode_text) in MODES {
        for (direction, before, after) in DIRECTIONS {
            for (quantifier, quantifier_text) in bounds() {
                for (selector, selector_text) in selectors(mode_text) {
                    let pattern = Pattern::new(VertexPattern::labeled(&["node"]))
                        .step(
                            EdgePattern::new(direction)
                                .labeled(&["link"])
                                .quantified(quantifier),
                            VertexPattern::labeled(&["node"]),
                        )
                        .mode(mode)
                        .select(selector);
                    let sql = format!(
                        "(s IS node) {before}[IS link]{after}{quantifier_text} (t IS node) \
                         {selector_text}"
                    );
                    let actual = world.query(&sql, "s.num AS s, t.num AS t");
                    compared += 1;
                    if actual != world.oracle(&pattern, ends) {
                        mismatches.push(sql);
                    }
                }
            }
        }
    }

    // Assert
    assert_eq!(mismatches, Vec::<String>::new());
    assert_eq!(compared, 180);
}

#[test]
pub(crate) fn should_agree_with_the_oracle_for_unbounded_quantifiers_under_finite_modes() {
    // Arrange
    let world = world();
    let mut mismatches = Vec::new();

    // Act
    for (mode, mode_text) in &MODES[1..] {
        for (direction, before, after) in DIRECTIONS {
            for (quantifier, quantifier_text) in
                [(Quantifier::star(), "*"), (Quantifier::plus(), "+")]
            {
                let pattern = Pattern::new(VertexPattern::labeled(&["node"]))
                    .step(
                        EdgePattern::new(direction)
                            .labeled(&["link"])
                            .quantified(quantifier),
                        VertexPattern::labeled(&["node"]),
                    )
                    .mode(*mode);
                let sql = format!(
                    "(s IS node) {before}[IS link]{after}{quantifier_text} (t IS node) \
                     KEEP ALL {mode_text} PATHS"
                );
                if world.query(&sql, "s.num AS s, t.num AS t") != world.oracle(&pattern, ends) {
                    mismatches.push(sql);
                }
            }
        }
    }

    // Assert
    assert_eq!(mismatches, Vec::<String>::new());
}
