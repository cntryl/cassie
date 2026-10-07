//! Ordered partial laws for the selected associative aggregate subset.
use super::{invalid_input, State};
use crate::executor::typed_batch::Cell;
use crate::executor::QueryError;

impl State {
    pub(super) fn empty_partial(&self) -> Self {
        match self {
            Self::Count(_) => Self::Count(0),
            Self::Sum { .. } => Self::Sum {
                sum: super::NumericSum::default(),
                seen: false,
            },
            Self::Avg { .. } => Self::Avg {
                sum: super::AvgSum::default(),
                count: 0,
            },
            Self::MinMax { max, .. } => Self::MinMax {
                selected: None,
                max: *max,
            },
        }
    }
    pub(super) fn merge_ordered(&mut self, later: &Self) -> Result<(), QueryError> {
        match (self, later) {
            (Self::Count(count), Self::Count(later)) => {
                *count = count
                    .checked_add(*later)
                    .ok_or_else(super::count_overflow)?;
            }
            (
                Self::Sum { sum, seen },
                Self::Sum {
                    sum: later,
                    seen: later_seen,
                },
            ) => {
                if sum.requires_row_order() || later.requires_row_order() {
                    return Err(row_order_required());
                }
                sum.merge(later);
                *seen |= later_seen;
            }
            (state @ Self::MinMax { .. }, Self::MinMax { selected, max }) => {
                let Self::MinMax {
                    max: current_max, ..
                } = state
                else {
                    unreachable!()
                };
                if current_max != max {
                    return Err(invalid_input());
                }
                if let Some(value) = selected {
                    state.update(Cell::Scalar(value))?;
                }
            }
            (Self::Avg { .. }, Self::Avg { .. }) => return Err(row_order_required()),
            _ => return Err(invalid_input()),
        }
        Ok(())
    }
}
fn row_order_required() -> QueryError {
    QueryError::General("typed aggregate requires row-order fold".to_owned())
}
