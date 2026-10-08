//! Retain fresh operator-bearing projections across private read handoffs.
use std::mem::size_of;
use std::sync::Arc;

use super::{batch, check_timeout, reserve_projection_output_before_building, QueryError};
use crate::executor::retained_memory::{add, grown_capacity, mul};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::sql::SelectItem;

pub(super) struct ProjectionOutputMemory {
    memory: QueryMemoryReservation,
    retain: bool,
    tighten: bool,
    scalar_boundary: bool,
    final_slots: usize,
}

impl ProjectionOutputMemory {
    pub(super) fn admit(
        controls: &QueryExecutionControls,
        batches: &[batch::Batch],
        projection: &[SelectItem],
        will_flatten: bool,
        retain_scalar: bool,
    ) -> Result<(Self, Option<QueryMemoryReservation>), QueryError> {
        let operator_input = batches
            .iter()
            .flatten()
            .any(|row| row.operator_memory().is_some());
        let copy = projection.iter().all(|item| match item {
            SelectItem::Wildcard
            | SelectItem::Column { .. }
            | SelectItem::WindowFunction { .. } => true,
            SelectItem::Function { function, .. } => {
                crate::sql::functions::is_aggregate_function(&function.name)
            }
            SelectItem::Expr { .. } => false,
        });
        let tighten = operator_input && copy;
        let retain = operator_input && (copy || retain_scalar);
        let scalar_boundary = operator_input && !copy;
        if tighten {
            check_timeout(controls)?;
        }
        let mut memory = reserve_projection_output_before_building(controls, batches, projection)?;
        let mut final_slots = 0;
        let mut scratch = None;
        if retain {
            if tighten && will_flatten {
                let rows = batches
                    .iter()
                    .try_fold(0, |rows, batch| add(rows, batch.len()))?;
                final_slots = mul(
                    mul(grown_capacity(rows, 4)?, 2)?,
                    size_of::<batch::BatchRow>(),
                )?;
                let slice_slots = batches.iter().try_fold(0, |bytes, batch| {
                    add(
                        bytes,
                        mul(
                            grown_capacity(batch.len(), 4)?,
                            size_of::<batch::BatchRow>(),
                        )?,
                    )
                })?;
                scratch = Some(controls.reserve_query_memory(add(
                    slice_slots,
                    mul(grown_capacity(batches.len(), 4)?, size_of::<batch::Batch>())?,
                )?)?);
            }
            memory.try_grow(add(final_slots, Self::arc_bytes())?)?;
        }
        Ok((
            Self {
                memory,
                retain,
                tighten,
                scalar_boundary,
                final_slots,
            },
            scratch,
        ))
    }

    pub(super) fn retain(
        mut self,
        controls: &QueryExecutionControls,
        batches: &mut Vec<batch::Batch>,
    ) -> Result<QueryMemoryReservation, QueryError> {
        if !self.retain {
            if self.scalar_boundary {
                crate::executor::typed_batch::relational_diagnostics::publish(
                    "projection",
                    "scalar_expression_projection",
                );
            }
            return Ok(self.memory);
        }
        if self.tighten {
            let retained = match check_timeout(controls)
                .and_then(|()| self.retained_bytes(batches, batches.capacity()))
            {
                Ok(retained) => retained,
                Err(error) => {
                    // The caller borrows this fresh buffer. Release its rows and
                    // outer backing while their construction admission is live.
                    *batches = Vec::new();
                    return Err(error);
                }
            };
            assert!(
                retained <= self.memory.bytes(),
                "projection retained backing must fit preadmission"
            );
            self.memory.shrink_to(retained);
        }
        let memory = Arc::new(self.memory);
        let attached = batches
            .iter_mut()
            .flatten()
            .try_for_each(|row| row.attach_operator_memory(controls, Arc::clone(&memory)));
        if let Err(error) = attached {
            *batches = Vec::new();
            return Err(error.into());
        }
        if self.scalar_boundary {
            crate::executor::typed_batch::relational_diagnostics::publish(
                "projection",
                "scalar_expression_projection",
            );
        }
        // Operator roots now own the retained reservation; keep the legacy caller's
        // local reservation type without charging an additional body copy.
        match controls.reserve_query_memory(0) {
            Ok(local) => Ok(local),
            Err(error) => {
                *batches = Vec::new();
                Err(error.into())
            }
        }
    }

    fn retained_bytes(
        &self,
        batches: &[batch::Batch],
        capacity: usize,
    ) -> Result<usize, QueryError> {
        batches
            .iter()
            .try_fold(
                add(
                    add(Self::arc_bytes(), self.final_slots)?,
                    mul(capacity, size_of::<batch::Batch>())?,
                )?,
                |bytes, batch| {
                    batch.iter().try_fold(
                        add(bytes, mul(batch.capacity(), size_of::<batch::BatchRow>())?)?,
                        |bytes, row| {
                            add(
                                bytes,
                                add(row.owned_body_bytes()?, row.pending_lookup_bytes()?)?,
                            )
                        },
                    )
                },
            )
            .map_err(QueryError::from)
    }

    const fn arc_bytes() -> usize {
        size_of::<QueryMemoryReservation>() + 2 * size_of::<usize>()
    }
}
