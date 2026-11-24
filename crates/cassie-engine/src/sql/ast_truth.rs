//! Native Boolean truth targets, independent of operand evaluation.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TruthValue {
    True,
    False,
    Unknown,
}
impl TruthValue {
    pub(crate) fn evaluate(self, value: Option<bool>, negated: bool) -> bool {
        let expected = match self {
            Self::True => Some(true),
            Self::False => Some(false),
            Self::Unknown => None,
        };
        (value == expected) != negated
    }
    pub(crate) const fn label(self, negated: bool) -> &'static str {
        match (self, negated) {
            (Self::True, false) => "IS TRUE",
            (Self::True, true) => "IS NOT TRUE",
            (Self::False, false) => "IS FALSE",
            (Self::False, true) => "IS NOT FALSE",
            (Self::Unknown, false) => "IS UNKNOWN",
            (Self::Unknown, true) => "IS NOT UNKNOWN",
        }
    }
}
