use super::{accounting::JoinRows, QueryError, SourceExecutionEnv};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum JoinRetentionPhase {
    NestedOutput,
    HashBuild,
    HashOutput,
    MergeKeyed,
    MergeOutput,
    BoundedSource,
    BoundedIndexedPoint,
    BoundedBuild,
    BoundedOutput,
    BoundedSampleKey,
}

/// The callback belongs to one invocation; it does not affect any sibling query.
#[derive(Default)]
pub(super) struct JoinRetentionContext<'a> {
    probe: Option<&'a dyn Fn(JoinRetentionPhase) -> Result<(), QueryError>>,
    entry: Option<&'a dyn Fn(JoinRetentionPhase)>,
}

#[cfg(test)]
impl<'a> JoinRetentionContext<'a> {
    pub(super) fn with_probe(
        probe: &'a dyn Fn(JoinRetentionPhase) -> Result<(), QueryError>,
    ) -> Self {
        Self {
            probe: Some(probe),
            entry: None,
        }
    }

    pub(super) fn with_entry_probe(
        entry: &'a dyn Fn(JoinRetentionPhase),
        probe: &'a dyn Fn(JoinRetentionPhase) -> Result<(), QueryError>,
    ) -> Self {
        Self {
            probe: Some(probe),
            entry: Some(entry),
        }
    }
}

impl JoinRetentionContext<'_> {
    pub(super) fn enter(&self, phase: JoinRetentionPhase) {
        if let Some(entry) = self.entry {
            entry(phase);
        }
    }

    pub(super) fn before(&self, phase: JoinRetentionPhase) -> Result<(), QueryError> {
        if let Some(probe) = self.probe {
            return probe(phase);
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
pub(super) enum PendingJoinDiagnostic {
    Typed {
        input_rows: crate::runtime::VectorizedJoinInputRows,
        matched_rows: usize,
        batch_size: usize,
        batches: usize,
    },
    Scalar {
        operator: &'static str,
        left_rows: usize,
        right_rows: usize,
        matched_rows: usize,
    },
    Vectorized {
        probe_rows: usize,
        build_rows: usize,
        matched_rows: usize,
        batch_size: usize,
        batches: usize,
    },
    VectorizedRoles {
        left_rows: usize,
        right_rows: usize,
        build_rows: usize,
        probe_rows: usize,
        matched_rows: usize,
        batch_size: usize,
        batches: usize,
    },
}

pub(super) struct JoinResult {
    pub(super) rows: JoinRows,
    diagnostic: Option<PendingJoinDiagnostic>,
    switch_state: Option<String>,
}

impl JoinResult {
    pub(super) const fn new(rows: JoinRows, diagnostic: PendingJoinDiagnostic) -> Self {
        Self {
            rows,
            diagnostic: Some(diagnostic),
            switch_state: None,
        }
    }

    pub(super) fn empty(env: &SourceExecutionEnv<'_>) -> Result<Self, QueryError> {
        Ok(Self {
            rows: JoinRows::try_new(env.controls)?,
            diagnostic: None,
            switch_state: None,
        })
    }

    pub(super) fn with_switch(mut self, state: String) -> Self {
        self.switch_state = Some(state);
        self
    }

    pub(super) fn into_parts(self) -> (JoinRows, Option<PendingJoinDiagnostic>, Option<String>) {
        (self.rows, self.diagnostic, self.switch_state)
    }
}

pub(super) fn publish(
    env: &SourceExecutionEnv<'_>,
    diagnostic: Option<PendingJoinDiagnostic>,
    switch_state: Option<String>,
    output_rows: usize,
) {
    match diagnostic {
        Some(PendingJoinDiagnostic::Typed {
            input_rows,
            matched_rows,
            batch_size,
            batches,
        }) => {
            env.cassie.runtime.record_typed_join_execution(
                input_rows,
                matched_rows,
                output_rows,
                batch_size,
                batches,
            );
        }
        Some(PendingJoinDiagnostic::Scalar {
            operator,
            left_rows,
            right_rows,
            matched_rows,
        }) => env.cassie.runtime.record_join_execution(
            operator,
            left_rows,
            right_rows,
            matched_rows,
            output_rows,
            None,
        ),
        Some(PendingJoinDiagnostic::Vectorized {
            probe_rows,
            build_rows,
            matched_rows,
            batch_size,
            batches,
        }) => env.cassie.runtime.record_vectorized_join_execution(
            probe_rows,
            build_rows,
            matched_rows,
            output_rows,
            batch_size,
            batches,
        ),
        Some(PendingJoinDiagnostic::VectorizedRoles {
            left_rows,
            right_rows,
            build_rows,
            probe_rows,
            matched_rows,
            batch_size,
            batches,
        }) => env
            .cassie
            .runtime
            .record_vectorized_join_execution_with_roles(
                crate::runtime::VectorizedJoinInputRows {
                    left: left_rows,
                    right: right_rows,
                    build: build_rows,
                    probe: probe_rows,
                },
                matched_rows,
                output_rows,
                batch_size,
                batches,
            ),
        None => {}
    }
    if let Some(state) = switch_state {
        env.cassie.runtime.record_runtime_operator_switch(
            super::VECTOR_TO_MERGE_SWITCH_PAIR,
            "row_threshold_exceeded",
            &state,
        );
    }
}
