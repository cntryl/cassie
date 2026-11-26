//! Bounded intermediate-cardinality heuristic; estimates never decide SQL truth.

use super::inputs::JoinInputs;

pub(super) fn order_cost(order: &[usize], inputs: &JoinInputs) -> u128 {
    checked_cost(order, inputs).unwrap_or_else(|| legacy_cost(order, &inputs.rows))
}

fn checked_cost(order: &[usize], inputs: &JoinInputs) -> Option<u128> {
    let mut intermediate = 1u128;
    let mut cost = 0u128;
    for (position, index) in order.iter().copied().enumerate() {
        #[cfg(test)]
        super::cost_observation_tests::record_checked_step();
        let raw = intermediate.checked_mul(u128::from(inputs.rows[index]))?;
        let prefix = &order[..position];
        let last = position + 1 == inputs.rows.len();
        let divisor = inputs
            .predicates
            .iter()
            .filter(|predicate| predicate.ready(prefix, index, last) == Some(true))
            .flat_map(|predicate| &predicate.equalities)
            .filter_map(|equality| equality.divisor(prefix, index, intermediate))
            .max()
            .unwrap_or(1);
        let quotient = raw / divisor;
        intermediate = quotient
            .checked_add(u128::from(!raw.is_multiple_of(divisor)))?
            .max(1);
        cost = cost.checked_add(intermediate)?;
    }
    Some(cost)
}

fn legacy_cost(order: &[usize], rows: &[u64]) -> u128 {
    let mut intermediate = 1u128;
    let mut cost = 0u128;
    for index in order {
        intermediate = intermediate.saturating_mul(u128::from(rows[*index]));
        cost = cost.saturating_add(intermediate);
    }
    cost
}
