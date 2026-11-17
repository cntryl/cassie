//! Transaction-owned projection write mode (`cassie.projection_write_mode`).
//!
//! The declaration and its sticky effect history live with the transaction, not
//! the session settings map, so they cannot leak through a connection pool. One
//! authority is shared by the session and every detached statement fork of the
//! same transaction; a new transaction gets a fresh one.

use super::{CassieError, CassieSession, SessionTransactionStatus};
use crate::app::TransactionErrorKind;
use crate::sql::ast::QueryStatement;
use parking_lot::Mutex;

/// The only name of the transaction-local projection write mode parameter.
pub(crate) const PROJECTION_WRITE_MODE: &str = "cassie.projection_write_mode";

pub(crate) fn is_projection_write_mode(name: &str) -> bool {
    name.trim().eq_ignore_ascii_case(PROJECTION_WRITE_MODE)
}

/// The declared mode. It is a request, not a durability receipt.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum ProjectionWriteMode {
    #[default]
    Default,
    Replayable,
    Strict,
}

impl ProjectionWriteMode {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Replayable => "replayable",
            Self::Strict => "strict",
        }
    }

    fn parse(value: &str) -> Result<Self, CassieError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "default" => Ok(Self::Default),
            "replayable" => Ok(Self::Replayable),
            "strict" => Ok(Self::Strict),
            _ => Err(CassieError::InvalidParameterValue(format!(
                "invalid value for parameter \"{PROJECTION_WRITE_MODE}\": \"{value}\"; \
                 expected default, replayable or strict"
            ))),
        }
    }
}

/// The policy a commit of this transaction's own Data writes must use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProjectionCommitPolicy {
    /// The existing policy for undeclared transactions.
    Existing,
    /// Buffered local WAL or the configured lazy cloud policy.
    Replayable,
    /// Local synchronous or cloud-strict publication: declared `strict`, or
    /// declared `replayable` with any non-replayable effect.
    Strict,
}

#[derive(Debug, Default)]
struct AuthorityState {
    mode: ProjectionWriteMode,
    /// A write or independently durable effect happened. Sticky: a savepoint
    /// rollback neither clears it nor reopens the declaration window.
    effect_seen: bool,
    /// An effect that is not a replayable row write happened. Sticky.
    strict_required: bool,
}

/// Generation-bound owner of one transaction's projection write declaration.
#[derive(Debug, Default)]
pub(super) struct ProjectionWriteAuthority {
    state: Mutex<AuthorityState>,
}

impl ProjectionWriteAuthority {
    pub(super) fn mode(&self) -> ProjectionWriteMode {
        self.state.lock().mode
    }

    /// Restores a savepoint's declaration; effect history stays sticky.
    pub(super) fn restore_mode(&self, mode: ProjectionWriteMode) {
        self.state.lock().mode = mode;
    }

    pub(super) fn note_effect(&self, strict: bool) {
        let mut state = self.state.lock();
        state.effect_seen = true;
        state.strict_required |= strict;
    }

    fn declare(&self, mode: ProjectionWriteMode, staged_effects: bool) -> Result<(), CassieError> {
        let mut state = self.state.lock();
        if state.effect_seen || staged_effects {
            return Err(CassieError::Transaction {
                kind: TransactionErrorKind::InvalidState,
                message: format!(
                    "parameter \"{PROJECTION_WRITE_MODE}\" must be set before the first write \
                     in the transaction"
                ),
            });
        }
        state.mode = mode;
        Ok(())
    }

    fn commit_policy(&self, strict_effects: bool) -> ProjectionCommitPolicy {
        let state = self.state.lock();
        match state.mode {
            ProjectionWriteMode::Default => ProjectionCommitPolicy::Existing,
            ProjectionWriteMode::Replayable if !state.strict_required && !strict_effects => {
                ProjectionCommitPolicy::Replayable
            }
            // A declared transaction with any non-replayable effect is promoted
            // to the strictest policy, never merely back to the default.
            ProjectionWriteMode::Replayable | ProjectionWriteMode::Strict => {
                ProjectionCommitPolicy::Strict
            }
        }
    }
}

fn transaction_only() -> CassieError {
    CassieError::Unsupported(format!(
        "parameter \"{PROJECTION_WRITE_MODE}\" can only be changed with SET LOCAL inside a \
         transaction block"
    ))
}

impl CassieSession {
    /// The declared mode; `default` outside a transaction.
    pub(crate) fn projection_write_mode(&self) -> ProjectionWriteMode {
        let transaction = self.transaction.lock();
        if transaction.status == SessionTransactionStatus::Idle {
            ProjectionWriteMode::Default
        } else {
            transaction.projection_write.mode()
        }
    }

    /// `SET LOCAL cassie.projection_write_mode`; `None` is `DEFAULT`.
    ///
    /// # Errors
    ///
    /// `25P01` outside an explicit transaction block, `25P02` in a failed
    /// block, `22023` for an unknown value and `25000` after the first write or
    /// independently durable effect of the transaction.
    pub(crate) fn set_projection_write_mode(&self, value: Option<&str>) -> Result<(), CassieError> {
        let transaction = self.transaction.lock();
        self.ensure_operation_current_locked(&transaction)?;
        match transaction.status {
            SessionTransactionStatus::InTransaction if !transaction.implicit => {}
            SessionTransactionStatus::Failed => return Err(CassieError::failed_transaction()),
            _ => {
                return Err(CassieError::Transaction {
                    kind: TransactionErrorKind::NoActiveTransaction,
                    message: format!(
                        "SET LOCAL \"{PROJECTION_WRITE_MODE}\" requires an explicit transaction \
                         block"
                    ),
                });
            }
        }
        let mode = value.map_or(Ok(ProjectionWriteMode::Default), ProjectionWriteMode::parse)?;
        let staged_effects = transaction.sealed_for_commit
            || !transaction.writes.is_empty()
            || transaction.catalog_overlay.is_some()
            || !transaction.conflict_intents.is_empty();
        transaction.projection_write.declare(mode, staged_effects)
    }

    /// Rejects every way of changing the mode other than `SET LOCAL`.
    pub(crate) fn reject_projection_write_mode_change(name: &str) -> Result<(), CassieError> {
        if is_projection_write_mode(name) {
            return Err(transaction_only());
        }
        Ok(())
    }

    /// Records a write (`strict == false`) or an effect that requires the
    /// existing strict policy. Shared with every fork of this transaction.
    pub(crate) fn note_projection_effect(&self, strict: bool) {
        let transaction = self.transaction.lock();
        if self.ensure_operation_current_locked(&transaction).is_ok() {
            transaction.projection_write.note_effect(strict);
        }
    }

    /// Classifies a statement at its execution boundary. Row DML is a
    /// replayable write. Reads, settings and transaction control are not
    /// effects. `COPY` stages rows and is a replayable write; a sequence
    /// advance in its defaults or query is classified where the value is
    /// recorded. Everything else, including DDL, security, procedure calls and
    /// EXPLAIN (which may execute its statement), conservatively promotes a
    /// declared transaction to the strict policy.
    pub(crate) fn note_statement_projection_effect(
        &self,
        statement: &QueryStatement,
    ) -> Result<(), CassieError> {
        let strict = match statement {
            QueryStatement::Select(_)
            | QueryStatement::Show(_)
            | QueryStatement::Set(_)
            | QueryStatement::Transaction(_) => return Ok(()),
            QueryStatement::Insert(_)
            | QueryStatement::Update(_)
            | QueryStatement::Delete(_)
            | QueryStatement::Copy(_) => false,
            _ => true,
        };
        let transaction = self.transaction.lock();
        self.ensure_operation_current_locked(&transaction)?;
        transaction.projection_write.note_effect(strict);
        Ok(())
    }

    /// The policy for committing this transaction's own Data writes.
    /// `strict_effects` reports effects only visible at commit, such as a
    /// staged transactional DDL overlay.
    pub(crate) fn projection_commit_policy(&self, strict_effects: bool) -> ProjectionCommitPolicy {
        let transaction = self.transaction.lock();
        if transaction.status == SessionTransactionStatus::Idle || transaction.implicit {
            return ProjectionCommitPolicy::Existing;
        }
        transaction
            .projection_write
            .commit_policy(strict_effects || transaction.catalog_overlay.is_some())
    }
}
