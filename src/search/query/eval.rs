//! Evaluation of a resolved query over one document's token positions.

use std::collections::BTreeSet;

use super::compile::PhraseLeaf;
use super::resolve::{Leaf, ResolvedNode, ResolvedQuery};

/// The token positions of one document, per analyzed term.
///
/// Positions are sorted and strictly increasing, and count every token the
/// tokenizer produced (removed stop words included).
pub trait DocumentPositions {
    fn positions(&self, term: &str) -> Option<&[u32]>;
}

#[derive(Default)]
struct Satisfied {
    terms: BTreeSet<String>,
    positions: BTreeSet<u32>,
    collect_positions: bool,
}

impl Satisfied {
    fn merge(&mut self, other: Self) {
        self.terms.extend(other.terms);
        self.positions.extend(other.positions);
    }

    fn scratch(&self) -> Self {
        Self {
            collect_positions: self.collect_positions,
            ..Self::default()
        }
    }
}

impl ResolvedQuery {
    /// The distinct index terms of the satisfied positive items, or `None`
    /// when the document does not satisfy the query.
    #[must_use]
    pub fn matched_terms(&self, document: &dyn DocumentPositions) -> Option<Vec<String>> {
        let root = self.root.as_ref()?;
        let mut satisfied = Satisfied::default();
        evaluate(root, document, &mut satisfied).then(|| satisfied.terms.into_iter().collect())
    }

    /// The sorted token positions that satisfied positive items; empty when the
    /// document does not satisfy the query.
    #[must_use]
    pub fn highlight_positions(&self, document: &dyn DocumentPositions) -> Vec<u32> {
        let Some(root) = self.root.as_ref() else {
            return Vec::new();
        };
        let mut satisfied = Satisfied {
            collect_positions: true,
            ..Satisfied::default()
        };
        if evaluate(root, document, &mut satisfied) {
            satisfied.positions.into_iter().collect()
        } else {
            Vec::new()
        }
    }
}

fn evaluate(node: &ResolvedNode, document: &dyn DocumentPositions, out: &mut Satisfied) -> bool {
    match node {
        ResolvedNode::Leaf(Leaf::AnyOf(terms)) => {
            let mut any = false;
            for term in terms {
                if let Some(positions) = document.positions(term).filter(|p| !p.is_empty()) {
                    any = true;
                    out.terms.insert(term.clone());
                    if out.collect_positions {
                        out.positions.extend(positions.iter().copied());
                    }
                }
            }
            any
        }
        ResolvedNode::Leaf(Leaf::Phrase(phrase)) => evaluate_phrase(phrase, document, out),
        ResolvedNode::Not(child) => {
            let mut scratch = out.scratch();
            !evaluate(child, document, &mut scratch)
        }
        ResolvedNode::And(children) => {
            let mut local = out.scratch();
            for child in children {
                if !evaluate(child, document, &mut local) {
                    return false;
                }
            }
            out.merge(local);
            true
        }
        ResolvedNode::Or(children) => {
            let mut any = false;
            for child in children {
                let mut local = out.scratch();
                if evaluate(child, document, &mut local) {
                    any = true;
                    out.merge(local);
                }
            }
            any
        }
    }
}

/// Smallest total slack of a chain, as `fwd + bwd` per position.
fn evaluate_phrase(
    phrase: &PhraseLeaf,
    document: &dyn DocumentPositions,
    out: &mut Satisfied,
) -> bool {
    let mut lists: Vec<&[u32]> = Vec::with_capacity(phrase.terms.len());
    for (_, term) in &phrase.terms {
        match document.positions(term) {
            Some(positions) if !positions.is_empty() => lists.push(positions),
            _ => return false,
        }
    }
    let slop = i64::from(phrase.slop);
    let nominal: Vec<i64> = phrase
        .terms
        .windows(2)
        .map(|pair| i64::from(pair[1].0) - i64::from(pair[0].0))
        .collect();
    let forward = chain_costs(&lists, &nominal, slop, Direction::Forward);
    let matched = forward
        .last()
        .is_some_and(|costs| costs.iter().any(Option::is_some));
    if !matched {
        return false;
    }
    out.terms
        .extend(phrase.terms.iter().map(|(_, term)| term.clone()));
    if out.collect_positions {
        let backward = chain_costs(&lists, &nominal, slop, Direction::Backward);
        for (index, list) in lists.iter().enumerate() {
            for (slot, position) in list.iter().enumerate() {
                if let (Some(before), Some(after)) = (forward[index][slot], backward[index][slot]) {
                    if before + after <= slop {
                        out.positions.insert(*position);
                    }
                }
            }
        }
    }
    true
}

#[derive(Clone, Copy)]
enum Direction {
    Forward,
    Backward,
}

/// For each term and position, the least extra slack of any partial chain that
/// ends (forward) or starts (backward) there, if at most `slop`.
fn chain_costs(
    lists: &[&[u32]],
    nominal: &[i64],
    slop: i64,
    direction: Direction,
) -> Vec<Vec<Option<i64>>> {
    let count = lists.len();
    let mut costs: Vec<Vec<Option<i64>>> =
        lists.iter().map(|list| vec![None; list.len()]).collect();
    let order: Vec<usize> = match direction {
        Direction::Forward => (0..count).collect(),
        Direction::Backward => (0..count).rev().collect(),
    };
    let Some((&seed, rest)) = order.split_first() else {
        return costs;
    };
    costs[seed].iter_mut().for_each(|cost| *cost = Some(0));
    let mut previous = seed;
    for &current in rest {
        // The gap between the pair, as the later term minus the earlier term.
        let gap = match direction {
            Direction::Forward => nominal[previous],
            Direction::Backward => nominal[current],
        };
        for (slot, position) in lists[current].iter().enumerate() {
            let position = i64::from(*position);
            // Neighbouring position in the already-costed list such that the
            // extra slack stays within bounds.
            let (low, high) = match direction {
                Direction::Forward => (position - gap - slop, position - gap),
                Direction::Backward => (position + gap, position + gap + slop),
            };
            let candidates = lists[previous];
            let start = candidates.partition_point(|candidate| i64::from(*candidate) < low);
            let mut best: Option<i64> = None;
            for (offset, candidate) in candidates[start..].iter().enumerate() {
                let candidate = i64::from(*candidate);
                if candidate > high {
                    break;
                }
                let extra = match direction {
                    Direction::Forward => position - candidate - gap,
                    Direction::Backward => candidate - position - gap,
                };
                if let Some(base) = costs[previous][start + offset] {
                    let total = base + extra;
                    if total <= slop && best.is_none_or(|current_best| total < current_best) {
                        best = Some(total);
                    }
                }
            }
            costs[current][slot] = best;
        }
        previous = current;
    }
    costs
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::search::analyzer::AnalyzerConfig;
    use crate::search::query::{
        CompiledQuery, SearchQueryInput, SearchSyntax, TermMatcher, Vocabulary,
    };

    struct Document(HashMap<String, Vec<u32>>);

    impl Document {
        fn of(text: &str) -> Self {
            let mut positions = HashMap::<String, Vec<u32>>::new();
            for (position, term) in AnalyzerConfig::default().analyze_positioned(text) {
                positions.entry(term).or_default().push(position);
            }
            Self(positions)
        }
    }

    impl DocumentPositions for Document {
        fn positions(&self, term: &str) -> Option<&[u32]> {
            self.0.get(term).map(Vec::as_slice)
        }
    }

    struct Terms(Vec<&'static str>);

    impl Vocabulary for Terms {
        fn matching_terms(
            &self,
            matchers: &[TermMatcher],
            limit: usize,
        ) -> Result<Vec<Vec<String>>, crate::app::CassieError> {
            Ok(crate::search::query::matching_terms_in(
                self.0.iter().copied(),
                matchers,
                limit,
            ))
        }
    }

    fn resolved(query: &str, vocabulary: &[&'static str]) -> ResolvedQuery {
        CompiledQuery::compile(
            &SearchQueryInput::new(query, SearchSyntax::Query),
            &AnalyzerConfig::default(),
        )
        .expect("valid query")
        .resolve(&Terms(vocabulary.to_vec()))
        .expect("resolvable query")
    }

    fn matches(query: &str, text: &str) -> bool {
        resolved(query, &[])
            .matched_terms(&Document::of(text))
            .is_some()
    }

    #[test]
    fn should_distinguish_adjacent_separated_with_reversed_words_given_a_phrase() {
        // Arrange
        let query = "\"red shoes\"";

        // Act
        let results = ["red shoes on sale", "red running shoes", "shoes red", "red"]
            .map(|text| matches(query, text));

        // Assert
        assert_eq!(results, [true, false, false, false]);
    }

    #[test]
    fn should_preserve_stop_word_gaps_given_a_phrase() {
        // Arrange
        let spaced = "\"alpha the beta\"";
        let tight = "\"alpha beta\"";

        // Act
        let results = [
            matches(spaced, "alpha the beta"),
            matches(spaced, "alpha beta"),
            matches(tight, "alpha the beta"),
            matches(tight, "alpha beta"),
        ];

        // Assert
        assert_eq!(results, [true, false, false, true]);
    }

    #[test]
    fn should_allow_ordered_slack_given_phrase_slop() {
        // Arrange
        let query = "\"alpha beta\"~2";

        // Act
        let results = [
            "alpha beta",
            "alpha x beta",
            "alpha x y beta",
            "alpha x y z beta",
            "beta alpha",
        ]
        .map(|text| matches(query, text));

        // Assert
        assert_eq!(results, [true, true, true, false, false]);
    }

    #[test]
    fn should_spend_total_slop_across_a_longer_phrase() {
        // Arrange
        let query = "\"a1 b2 c3\"~1";

        // Act
        let results = ["a1 b2 c3", "a1 x b2 c3", "a1 b2 x c3", "a1 x b2 x c3"]
            .map(|text| matches(query, text));

        // Assert
        assert_eq!(results, [true, true, true, false]);
    }

    #[test]
    fn should_evaluate_boolean_structure_over_terms() {
        // Arrange
        let text = "red running shoes";

        // Act
        let results = [
            matches("red shoes", text),
            matches("red boots", text),
            matches("red OR boots", text),
            matches("boots OR blue", text),
            matches("shoes -blue", text),
            matches("shoes -red", text),
            matches("shoes NOT (blue OR red)", text),
            matches("(red OR blue) (shoes OR boots)", text),
        ];

        // Assert
        assert_eq!(
            results,
            [true, false, true, false, true, false, false, true]
        );
    }

    #[test]
    fn should_expand_prefix_with_typo_items_against_the_vocabulary() {
        // Arrange
        let vocabulary = ["color", "colour", "collar", "shoes"];
        let prefix = resolved("col*", &vocabulary);
        let typo = resolved("colour~", &vocabulary);

        // Act
        let prefix_terms = prefix.matched_terms(&Document::of("colour and collar"));
        let typo_terms = typo.matched_terms(&Document::of("what color is it"));

        // Assert
        assert_eq!(
            prefix_terms,
            Some(vec!["collar".to_string(), "colour".to_string()])
        );
        assert_eq!(typo_terms, Some(vec!["color".to_string()]));
        assert_eq!(prefix.positive_terms(), ["collar", "color", "colour"]);
    }

    #[test]
    fn should_score_only_satisfied_positive_terms() {
        // Arrange
        let query = resolved("(red OR blue) shoes -boots", &[]);

        // Act
        let terms = query.matched_terms(&Document::of("red shoes and green hats"));

        // Assert
        assert_eq!(terms, Some(vec!["red".to_string(), "shoes".to_string()]));
    }

    #[test]
    fn should_highlight_only_positions_that_take_part_in_a_phrase() {
        // Arrange
        let query = resolved("\"red shoes\"", &[]);
        let document = Document::of("red hat red shoes shoes");

        // Act
        let positions = query.highlight_positions(&document);

        // Assert
        assert_eq!(positions, vec![2, 3]);
    }

    #[test]
    fn should_not_highlight_negated_items_or_non_matching_documents() {
        // Arrange
        let query = resolved("shoes -boots", &[]);

        // Act
        let kept = query.highlight_positions(&Document::of("red shoes"));
        let excluded = query.highlight_positions(&Document::of("red shoes boots"));

        // Assert
        assert_eq!(kept, vec![1]);
        assert_eq!(excluded, Vec::<u32>::new());
    }

    #[test]
    fn should_return_the_expansion_limit_error_given_an_excessive_prefix() {
        // Arrange
        let vocabulary: Vec<&'static str> = (0..200)
            .map(|index| &*Box::leak(format!("pre{index:03}").into_boxed_str()))
            .collect();
        let compiled = CompiledQuery::compile(
            &SearchQueryInput::new("pre*", SearchSyntax::Query),
            &AnalyzerConfig::default(),
        )
        .expect("valid query");

        // Act
        let result = compiled.resolve(&Terms(vocabulary));

        // Assert
        let Err(crate::app::CassieError::ResourceLimit(message)) = result else {
            panic!("expected a resource limit error");
        };
        assert!(message.contains("max_expansions_per_item"));
    }
}
