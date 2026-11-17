use std::ffi::OsStr;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReplayMode {
    Strict,
    Buffered,
    /// An explicit SQL transaction with no declared projection write mode.
    SqlDefault,
    /// `SET LOCAL cassie.projection_write_mode = 'replayable'`.
    SqlReplayable,
}

impl ReplayMode {
    pub(crate) fn parse(value: Option<&OsStr>) -> Result<Self, &'static str> {
        match value {
            None => Ok(Self::Strict),
            Some(value) if value == OsStr::new("local_strict") => Ok(Self::Strict),
            Some(value) if value == OsStr::new("local_buffered") => Ok(Self::Buffered),
            Some(value) if value == OsStr::new("sql_default") => Ok(Self::SqlDefault),
            Some(value) if value == OsStr::new("sql_replayable") => Ok(Self::SqlReplayable),
            Some(_) => Err(
                "CASSIE_REPLAY_BENCH_MODE must be local_strict, local_buffered, sql_default or sql_replayable",
            ),
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Strict => "local_strict",
            Self::Buffered => "local_buffered",
            Self::SqlDefault => "sql_default",
            Self::SqlReplayable => "sql_replayable",
        }
    }

    pub(crate) const fn api(self) -> &'static str {
        match self {
            Self::Strict => "replay_projection_batch_strict",
            Self::Buffered => "replay_projection_batch",
            Self::SqlDefault => "sql_transaction",
            Self::SqlReplayable => "sql_transaction_set_local_replayable",
        }
    }

    pub(crate) const fn native_policy(self) -> &'static str {
        match self {
            Self::Strict | Self::SqlDefault => "local_sync",
            Self::Buffered | Self::SqlReplayable => "local_buffered",
        }
    }

    pub(crate) const fn is_sql(self) -> bool {
        matches!(self, Self::SqlDefault | Self::SqlReplayable)
    }
}

/// Whether the measured table carries a scalar index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shape {
    Indexed,
    Unindexed,
}

impl Shape {
    pub(crate) fn parse(value: Option<&OsStr>) -> Result<Self, &'static str> {
        match value {
            None => Ok(Self::Indexed),
            Some(value) if value == OsStr::new("indexed") => Ok(Self::Indexed),
            Some(value) if value == OsStr::new("unindexed") => Ok(Self::Unindexed),
            Some(_) => Err("CASSIE_REPLAY_BENCH_SHAPE must be indexed or unindexed"),
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Indexed => "indexed",
            Self::Unindexed => "unindexed",
        }
    }
}

/// Separate API workloads; SQL lanes have no replay event ledger.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Workload {
    Apply,
    Duplicate,
    Noop,
}

impl Workload {
    pub(crate) fn parse(value: Option<&OsStr>) -> Result<Self, &'static str> {
        match value {
            None => Ok(Self::Apply),
            Some(value) if value == OsStr::new("apply") => Ok(Self::Apply),
            Some(value) if value == OsStr::new("duplicate") => Ok(Self::Duplicate),
            Some(value) if value == OsStr::new("noop") => Ok(Self::Noop),
            Some(_) => Err("CASSIE_REPLAY_BENCH_WORKLOAD must be apply, duplicate or noop"),
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Apply => "apply",
            Self::Duplicate => "duplicate",
            Self::Noop => "noop",
        }
    }

    pub(crate) const fn validate(self, mode: ReplayMode) -> Result<(), &'static str> {
        if mode.is_sql() && !matches!(self, Self::Apply) {
            return Err(
                "SQL projector lanes support only apply; duplicate/noop measure the replay API",
            );
        }
        Ok(())
    }

    pub(crate) const fn expected_counts(self, batch_size: u64) -> (u64, u64) {
        match self {
            Self::Apply => (batch_size, 0),
            Self::Duplicate => (0, batch_size),
            Self::Noop => (0, 0),
        }
    }
}
