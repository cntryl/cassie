#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperatorSwitchingEnabled(bool);

impl OperatorSwitchingEnabled {
    #[must_use]
    pub const fn disabled() -> Self {
        Self(false)
    }

    #[must_use]
    pub const fn enabled() -> Self {
        Self(true)
    }

    #[must_use]
    pub const fn is_enabled(self) -> bool {
        self.0
    }
}

impl Default for OperatorSwitchingEnabled {
    fn default() -> Self {
        Self::disabled()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionResultCacheEnabled(bool);

impl ExecutionResultCacheEnabled {
    #[must_use]
    pub const fn disabled() -> Self {
        Self(false)
    }

    #[must_use]
    pub const fn enabled() -> Self {
        Self(true)
    }

    #[must_use]
    pub const fn is_enabled(self) -> bool {
        self.0
    }
}

impl Default for ExecutionResultCacheEnabled {
    fn default() -> Self {
        Self::enabled()
    }
}
