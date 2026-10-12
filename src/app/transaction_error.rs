//! Typed transaction and lock error identities shared by application transports.

/// PostgreSQL transaction and lock failure class, independent of message text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TransactionErrorKind {
    /// The transaction state does not permit the requested operation.
    InvalidState,
    /// An operation requires execution outside a transaction block.
    ActiveTransaction,
    /// An operation requires an active transaction block.
    NoActiveTransaction,
    /// The current transaction has failed and requires recovery.
    FailedTransaction,
    /// A write is forbidden in the current read-only transaction.
    ReadOnlyTransaction,
    /// A requested savepoint does not exist.
    InvalidSavepoint,
    /// Serialization could not be preserved; retry the entire transaction.
    SerializationFailure,
    /// A wait cycle requires aborting a transaction.
    DeadlockDetected,
    /// A requested lock cannot be acquired under its wait policy.
    LockNotAvailable,
}

impl TransactionErrorKind {
    pub(crate) const fn sql_state(self) -> &'static str {
        match self {
            Self::InvalidState => "25000",
            Self::ActiveTransaction => "25001",
            Self::NoActiveTransaction => "25P01",
            Self::FailedTransaction => "25P02",
            Self::ReadOnlyTransaction => "25006",
            Self::InvalidSavepoint => "3B001",
            Self::SerializationFailure => "40001",
            Self::DeadlockDetected => "40P01",
            Self::LockNotAvailable => "55P03",
        }
    }

    pub(crate) const fn http_status(self) -> u16 {
        match self {
            Self::SerializationFailure | Self::DeadlockDetected | Self::LockNotAvailable => 409,
            Self::InvalidState
            | Self::ActiveTransaction
            | Self::NoActiveTransaction
            | Self::FailedTransaction
            | Self::ReadOnlyTransaction
            | Self::InvalidSavepoint => 400,
        }
    }
}
