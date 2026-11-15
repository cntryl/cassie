use std::ffi::OsStr;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReplayMode {
    Strict,
    Buffered,
}

impl ReplayMode {
    pub(crate) fn parse(value: Option<&OsStr>) -> Result<Self, &'static str> {
        match value {
            None => Ok(Self::Strict),
            Some(value) if value == OsStr::new("local_strict") => Ok(Self::Strict),
            Some(value) if value == OsStr::new("local_buffered") => Ok(Self::Buffered),
            Some(_) => Err("CASSIE_REPLAY_BENCH_MODE must be local_strict or local_buffered"),
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Strict => "local_strict",
            Self::Buffered => "local_buffered",
        }
    }

    pub(crate) const fn api(self) -> &'static str {
        match self {
            Self::Strict => "replay_projection_batch_strict",
            Self::Buffered => "replay_projection_batch",
        }
    }

    pub(crate) const fn native_policy(self) -> &'static str {
        match self {
            Self::Strict => "local_sync",
            Self::Buffered => "local_buffered",
        }
    }
}
