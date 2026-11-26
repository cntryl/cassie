//! Bounded intermediate-cardinality heuristic; estimates never decide SQL truth.

use super::inputs::JoinInputs;

pub(super) fn order_cost(order: &[usize], inputs: &JoinInputs) -> u128 {
    checked_cost(order, inputs).unwrap_or_else(|| legacy_cost(order, &inputs.rows))
}

#[derive(Clone, Copy)]
pub(super) struct CheckedPrefix {
    intermediate: u128,
    pub(super) total: u128,
}

impl CheckedPrefix {
    pub(super) const fn empty() -> Self {
        Self {
            intermediate: 1,
            total: 0,
        }
    }

    pub(super) fn extend(
        self,
        prefix: &[usize],
        index: usize,
        inputs: &JoinInputs,
    ) -> Option<Self> {
        #[cfg(test)]
        super::cost_observation_tests::record_checked_step();
        let raw = self
            .intermediate
            .checked_mul(u128::from(inputs.rows[index]))?;
        let last = prefix.len() + 1 == inputs.rows.len();
        let divisor = inputs
            .predicates
            .iter()
            .filter(|predicate| predicate.ready(prefix, index, last) == Some(true))
            .flat_map(|predicate| &predicate.equalities)
            .filter_map(|equality| equality.divisor(prefix, index, self.intermediate))
            .max()
            .unwrap_or(1);
        let quotient = raw / divisor;
        let intermediate = quotient
            .checked_add(u128::from(!raw.is_multiple_of(divisor)))?
            .max(1);
        Some(Self {
            intermediate,
            total: self.total.checked_add(intermediate)?,
        })
    }
}

fn checked_cost(order: &[usize], inputs: &JoinInputs) -> Option<u128> {
    let mut score = CheckedPrefix::empty();
    for (position, index) in order.iter().copied().enumerate() {
        score = score.extend(&order[..position], index, inputs)?;
    }
    Some(score.total)
}

pub(super) fn legacy_cost(order: &[usize], rows: &[u64]) -> u128 {
    let mut intermediate = 1u128;
    let mut cost = 0u128;
    for index in order {
        intermediate = intermediate.saturating_mul(u128::from(rows[*index]));
        cost = cost.saturating_add(intermediate);
    }
    cost
}
