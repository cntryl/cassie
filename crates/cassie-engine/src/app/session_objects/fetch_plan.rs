//! Cursor movement for every `FETCH`/`MOVE` direction, over a cursor with
//! `rows` rows whose position is 0 before the first row and `rows + 1`
//! after the last, as in PostgreSQL.

use crate::sql::ast::FetchDirection;

use super::SessionObjectError;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct FetchPlan {
    /// Zero-based row indexes, in the order they are returned.
    pub(super) rows: Vec<usize>,
    pub(super) position: usize,
}

/// Plans one `FETCH`; a cursor without `SCROLL` rejects backward movement.
pub(super) fn plan_fetch(
    direction: FetchDirection,
    position: usize,
    rows: usize,
    scroll: bool,
) -> Result<FetchPlan, SessionObjectError> {
    let after_end = rows + 1;
    let position = position.min(after_end);
    match direction {
        FetchDirection::Forward(Some(0))
        | FetchDirection::Backward(Some(0))
        | FetchDirection::Relative(0) => refetch(position, rows, scroll),
        FetchDirection::Forward(count) => {
            let available = rows.saturating_sub(position);
            let wanted = count.map_or(usize::MAX, count_to_usize);
            let take = wanted.min(available);
            let next = if take < wanted {
                after_end
            } else {
                position + take
            };
            Ok(FetchPlan {
                rows: (position..position + take).collect(),
                position: next,
            })
        }
        FetchDirection::Backward(count) => {
            require_scroll(scroll)?;
            let available = position.saturating_sub(1).min(rows);
            let wanted = count.map_or(usize::MAX, count_to_usize);
            let take = wanted.min(available);
            let next = if take < wanted { 0 } else { position - take };
            Ok(FetchPlan {
                rows: (position - take..position)
                    .rev()
                    .map(|row| row - 1)
                    .collect(),
                position: next,
            })
        }
        FetchDirection::Absolute(count) => {
            if !scroll && count < 0 {
                return Err(SessionObjectError::scan_forward_only());
            }
            let target = if count < 0 {
                i64::try_from(after_end).unwrap_or(i64::MAX) + count
            } else {
                count
            };
            seek(target, position, rows, scroll)
        }
        FetchDirection::Relative(count) => {
            if !scroll && count < 0 {
                return Err(SessionObjectError::scan_forward_only());
            }
            seek(
                i64::try_from(position)
                    .unwrap_or(i64::MAX)
                    .saturating_add(count),
                position,
                rows,
                scroll,
            )
        }
    }
}

fn count_to_usize(count: i64) -> usize {
    usize::try_from(count).unwrap_or(usize::MAX)
}

fn require_scroll(scroll: bool) -> Result<(), SessionObjectError> {
    if scroll {
        Ok(())
    } else {
        Err(SessionObjectError::scan_forward_only())
    }
}

fn refetch(position: usize, rows: usize, scroll: bool) -> Result<FetchPlan, SessionObjectError> {
    if (1..=rows).contains(&position) {
        require_scroll(scroll)?;
        return Ok(FetchPlan {
            rows: vec![position - 1],
            position,
        });
    }
    Ok(FetchPlan {
        rows: Vec::new(),
        position,
    })
}

/// Moves to absolute position `target`, returning its row when it exists.
fn seek(
    target: i64,
    position: usize,
    rows: usize,
    scroll: bool,
) -> Result<FetchPlan, SessionObjectError> {
    let after_end = rows + 1;
    let target = usize::try_from(target.max(0))
        .unwrap_or(usize::MAX)
        .min(after_end);
    if target == position {
        return refetch(position, rows, scroll);
    }
    if target < position {
        require_scroll(scroll)?;
    }
    Ok(FetchPlan {
        rows: if (1..=rows).contains(&target) {
            vec![target - 1]
        } else {
            Vec::new()
        },
        position: target,
    })
}

#[cfg(test)]
mod tests {
    use super::{plan_fetch, FetchDirection};

    /// Replays a script against five rows, returning 1-based values.
    fn replay(scroll: bool, script: &[FetchDirection]) -> Vec<Option<Vec<usize>>> {
        let mut position = 0;
        script
            .iter()
            .map(|direction| {
                plan_fetch(*direction, position, 5, scroll)
                    .ok()
                    .map(|plan| {
                        position = plan.position;
                        plan.rows.iter().map(|row| row + 1).collect()
                    })
            })
            .collect()
    }

    #[test]
    fn should_follow_postgresql_scroll_cursor_positions_given_every_direction() {
        // Arrange
        let script = [
            FetchDirection::Forward(Some(2)),
            FetchDirection::Absolute(1),
            FetchDirection::Absolute(-1),
            FetchDirection::Forward(None),
            FetchDirection::Relative(0),
            FetchDirection::Backward(None),
            FetchDirection::Backward(Some(1)),
            FetchDirection::Absolute(-2),
            FetchDirection::Absolute(100),
            FetchDirection::Relative(0),
            FetchDirection::Backward(Some(1)),
            FetchDirection::Forward(None),
            FetchDirection::Backward(Some(2)),
            FetchDirection::Forward(Some(1)),
        ];

        // Act
        let results = replay(true, &script);

        // Assert
        assert_eq!(
            results,
            [
                Some(vec![1, 2]),
                Some(vec![1]),
                Some(vec![5]),
                Some(vec![]),
                Some(vec![]),
                Some(vec![5, 4, 3, 2, 1]),
                Some(vec![]),
                Some(vec![4]),
                Some(vec![]),
                Some(vec![]),
                Some(vec![5]),
                Some(vec![]),
                Some(vec![5, 4]),
                Some(vec![5]),
            ]
        );
    }

    #[test]
    fn should_reject_backward_movement_given_a_no_scroll_cursor() {
        // Arrange
        let script = [
            FetchDirection::Forward(Some(0)),
            FetchDirection::Absolute(2),
            FetchDirection::Relative(2),
            FetchDirection::Absolute(-1),
            FetchDirection::Relative(0),
            FetchDirection::Backward(Some(1)),
            FetchDirection::Absolute(1),
            FetchDirection::Forward(None),
        ];

        // Act
        let results = replay(false, &script);

        // Assert
        assert_eq!(
            results,
            [
                Some(vec![]),
                Some(vec![2]),
                Some(vec![4]),
                None,
                None,
                None,
                None,
                Some(vec![5]),
            ]
        );
    }
}
