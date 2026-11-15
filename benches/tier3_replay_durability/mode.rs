use std::ffi::OsStr;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReplayMode {
    Strict,
    Buffered,
}

impl ReplayMode {
    fn parse(value: Option<&OsStr>) -> Result<Self, &'static str> {
        match value {
            None => Ok(Self::Strict),
            Some(value) if value == OsStr::new("local_strict") => Ok(Self::Strict),
            Some(value) if value == OsStr::new("local_buffered") => Ok(Self::Buffered),
            Some(_) => Err("CASSIE_REPLAY_BENCH_MODE must be local_strict or local_buffered"),
        }
    }

    pub(crate) fn from_environment() -> Self {
        Self::parse(std::env::var_os("CASSIE_REPLAY_BENCH_MODE").as_deref())
            .unwrap_or_else(|error| panic!("{error}"))
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

#[cfg(test)]
mod tests {
    use super::{OsStr, ReplayMode};

    #[test]
    fn should_default_to_strict_replay_mode() {
        // Arrange
        let input = None;
        // Act
        let mode = ReplayMode::parse(input);
        // Assert
        assert_eq!(mode, Ok(ReplayMode::Strict));
    }

    #[test]
    fn should_select_each_explicit_replay_mode() {
        // Arrange
        let modes = [OsStr::new("local_strict"), OsStr::new("local_buffered")];
        // Act
        let actual = modes.map(|mode| ReplayMode::parse(Some(mode)));
        // Assert
        assert_eq!(actual, [Ok(ReplayMode::Strict), Ok(ReplayMode::Buffered)]);
    }

    #[test]
    fn should_reject_unknown_replay_mode() {
        // Arrange
        let mode = OsStr::new("buffered");
        // Act
        let actual = ReplayMode::parse(Some(mode));
        // Assert
        assert!(actual.is_err());
    }

    #[cfg(unix)]
    #[test]
    fn should_reject_non_unicode_replay_mode() {
        // Arrange
        use std::os::unix::ffi::OsStringExt;
        let mode = std::ffi::OsString::from_vec(vec![0xff]);
        // Act
        let actual = ReplayMode::parse(Some(&mode));
        // Assert
        assert!(actual.is_err());
    }
}
