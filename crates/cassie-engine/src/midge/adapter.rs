//! Engine compatibility facade for the storage-owned Midge adapter.

pub use cassie_storage::adapter::*;
pub use cassie_storage::adapter::{canonical_field_value, index_key_field_value};
pub use cassie_storage::adapter::{ScalarIndexBound, ScalarIndexScanHit, ScalarIndexScanRequest};
pub(crate) use crate::app::{StatementDataRead, StatementReadScope};

pub mod column_batch_format_v1;
pub use column_batch_format_v1::{
    decode_column_batch_manifest_for_test, encode_column_batch_manifest_for_test,
};

#[cfg(test)]
#[path = "adapter/native_memory_tests.rs"]
mod native_memory_tests;
#[cfg(test)]
#[path = "adapter/legacy_graph_state_tests.rs"]
mod legacy_graph_state_tests;

use std::cell::RefCell;

thread_local! {
    static CURRENT_ENGINE_QUERY_CONTROLS: RefCell<Option<crate::runtime::QueryExecutionControls>> = const { RefCell::new(None) };
}

/// Engine compatibility scope that installs the same request authority in the
/// Engine and the concrete storage publication coordinator.
#[doc(hidden)]
pub struct PublicationControlScope {
    previous: Option<crate::runtime::QueryExecutionControls>,
    restore_engine_controls: bool,
    _storage: cassie_storage::adapter::NativePublicationControlScope,
}

impl PublicationControlScope {
    pub(crate) fn check_interruption(
        controls: &crate::runtime::QueryExecutionControls,
    ) -> Result<(), crate::app::CassieError> {
        if let Some(current) = Self::current_query_controls() {
            current.check_interruption()?;
        }
        controls.check_interruption()
    }

    pub(crate) fn for_request(
        limits: &crate::config::CassieRuntimeLimits,
        cancellation: Option<&crate::runtime::QueryCancellationHandle>,
    ) -> Self {
        if let Some(controls) = Self::current_query_controls() {
            return Self::enter(Some(&controls));
        }
        let controls = crate::runtime::QueryExecutionControls::with_cancellation(
            limits,
            std::time::Instant::now(),
            cancellation.cloned().unwrap_or_default(),
        );
        Self::enter(Some(&controls))
    }

    #[doc(hidden)]
    pub fn enter_for_rest(
        cancellation: &crate::runtime::QueryCancellationHandle,
        deadline: std::time::Instant,
    ) -> Self {
        Self {
            previous: None,
            restore_engine_controls: false,
            _storage: cassie_storage::adapter::NativePublicationControlScope::enter_for_rest(
                cancellation.request_cancellation(),
                deadline,
            ),
        }
    }

    pub(crate) fn enter_for_replay(
        cancellation: &crate::runtime::QueryCancellationHandle,
    ) -> Self {
        Self {
            previous: None,
            restore_engine_controls: false,
            _storage: cassie_storage::adapter::NativePublicationControlScope::enter_for_replay(
                cancellation.request_cancellation(),
            ),
        }
    }

    pub(crate) fn check_current() -> Result<(), crate::app::CassieError> {
        if let Some(controls) = Self::current_query_controls() {
            controls.check_interruption()?;
        }
        cassie_storage::adapter::NativePublicationControlScope::check_current().map_err(Into::into)
    }

    pub(crate) fn current_query_controls() -> Option<crate::runtime::QueryExecutionControls> {
        CURRENT_ENGINE_QUERY_CONTROLS.with(|current| current.borrow().clone())
    }

    pub(crate) fn enter(controls: Option<&crate::runtime::QueryExecutionControls>) -> Self {
        let previous = CURRENT_ENGINE_QUERY_CONTROLS.with(|current| current.borrow().clone());
        let active = previous.clone().or_else(|| controls.cloned());
        let storage_context = active.as_ref().map(crate::runtime::QueryExecutionControls::storage_request);
        let storage = cassie_storage::adapter::NativePublicationControlScope::enter(
            storage_context.as_ref(),
        );
        CURRENT_ENGINE_QUERY_CONTROLS.with(|current| current.replace(active));
        Self {
            previous,
            restore_engine_controls: true,
            _storage: storage,
        }
    }

    pub(crate) fn terminal_completion() -> Self {
        let previous = CURRENT_ENGINE_QUERY_CONTROLS.with(|current| current.replace(None));
        Self {
            previous,
            restore_engine_controls: true,
            _storage: cassie_storage::adapter::NativePublicationControlScope::terminal_completion(),
        }
    }
}

impl Drop for PublicationControlScope {
    fn drop(&mut self) {
        if self.restore_engine_controls {
            CURRENT_ENGINE_QUERY_CONTROLS.with(|current| current.replace(self.previous.take()));
        }
    }
}

#[cfg(test)]
pub(crate) mod statement_read {
    #[path = "adapter/statement_read/tests.rs"]
    mod tests;
}

#[cfg(test)]
pub(crate) mod query_scan_control {
    pub use cassie_storage::adapter::{
        current_query_scan_control_scope, query_scan_control_test_guard,
        set_query_scan_cancellation_after_entries, should_cancel_controlled_query_scan,
    };

    #[path = "adapter/query_scan_control_tests.rs"]
    mod tests;
}

impl From<cassie_storage::StorageError> for crate::app::CassieError {
    fn from(error: cassie_storage::StorageError) -> Self {
        use cassie_storage::StorageError;

        match error {
            StorageError::Native(error) => Self::from(error),
            StorageError::Codec(error) => Self::from(error),
            StorageError::Parse(message) => Self::Parse(message),
            StorageError::Execution(message) => Self::Execution(message),
            StorageError::InvalidQuery(message) => Self::InvalidQuery(message),
            StorageError::InvalidParameterValue(message) => Self::InvalidParameterValue(message),
            StorageError::Unsupported(message) => Self::Unsupported(message),
            StorageError::InvalidVector(message) => Self::InvalidVector(message),
            StorageError::ResourceLimit(message) => Self::ResourceLimit(message),
            StorageError::QueryCanceled => Self::QueryCanceled,
            StorageError::DeadlineExceeded => Self::DeadlineExceeded,
            StorageError::CollectionNotFound(name) => Self::CollectionNotFound(name),
            StorageError::NotFound(message) => Self::NotFound(message),
            StorageError::Storage(message) => Self::Storage(message),
            StorageError::StorageBootstrap(message) => Self::StorageBootstrap(message),
            StorageError::StorageMissingFamily(message) => Self::StorageMissingFamily(message),
            StorageError::OutcomeUnknown(message) | StorageError::RecoveryRequired(message) => {
                Self::Storage(message)
            }
            StorageError::StorageRetryable(error) => Self::StorageRetryable(
                crate::app::RetryableStorageError::new(error.kind().into(), error.to_string()),
            ),
            StorageError::CatalogObjectNotFound { kind, name } => {
                Self::CatalogObjectNotFound {
                    kind: kind.into(),
                    name,
                }
            }
            StorageError::CatalogObjectAlreadyExists { kind, name } => {
                Self::CatalogObjectAlreadyExists {
                    kind: kind.into(),
                    name,
                }
            }
            StorageError::UniqueViolation {
                table,
                column,
                constraint,
            } => Self::UniqueViolation {
                table,
                column,
                constraint,
            },
            StorageError::Transaction { kind, message } => Self::Transaction {
                kind: kind.into(),
                message,
            },
            StorageError::Diagnostic {
                sql_state,
                message,
                detail,
                hint,
            } => {
                let mut diagnostic = crate::app::DiagnosticError::new(sql_state, message);
                if let Some(detail) = detail {
                    diagnostic = diagnostic.with_detail(detail);
                }
                if let Some(hint) = hint {
                    diagnostic = diagnostic.with_hint(hint);
                }
                Self::from(diagnostic)
            }
            StorageError::EngineCallback(error) => match error.downcast::<crate::app::CassieError>() {
                Ok(error) => *error,
                Err(error) => Self::Execution(error.to_string()),
            },
        }
    }
}

impl From<cassie_storage::adapter::error::CatalogObjectKind> for crate::app::CatalogObjectKind {
    fn from(kind: cassie_storage::adapter::error::CatalogObjectKind) -> Self {
        match kind {
            cassie_storage::adapter::error::CatalogObjectKind::Database => Self::Database,
            cassie_storage::adapter::error::CatalogObjectKind::Relation => Self::Relation,
            cassie_storage::adapter::error::CatalogObjectKind::Schema => Self::Schema,
            cassie_storage::adapter::error::CatalogObjectKind::Index => Self::Index,
            cassie_storage::adapter::error::CatalogObjectKind::View => Self::View,
            cassie_storage::adapter::error::CatalogObjectKind::Role => Self::Role,
            cassie_storage::adapter::error::CatalogObjectKind::Sequence => Self::Sequence,
            cassie_storage::adapter::error::CatalogObjectKind::Rollup => Self::Rollup,
            cassie_storage::adapter::error::CatalogObjectKind::RetentionPolicy => {
                Self::RetentionPolicy
            }
            cassie_storage::adapter::error::CatalogObjectKind::ProjectionVersion => {
                Self::ProjectionVersion
            }
        }
    }
}

impl From<cassie_storage::adapter::error::TransactionErrorKind>
    for crate::app::TransactionErrorKind
{
    fn from(kind: cassie_storage::adapter::error::TransactionErrorKind) -> Self {
        match kind {
            cassie_storage::adapter::error::TransactionErrorKind::InvalidState => {
                Self::InvalidState
            }
            cassie_storage::adapter::error::TransactionErrorKind::ActiveTransaction => {
                Self::ActiveTransaction
            }
            cassie_storage::adapter::error::TransactionErrorKind::NoActiveTransaction => {
                Self::NoActiveTransaction
            }
            cassie_storage::adapter::error::TransactionErrorKind::FailedTransaction => {
                Self::FailedTransaction
            }
            cassie_storage::adapter::error::TransactionErrorKind::ReadOnlyTransaction => {
                Self::ReadOnlyTransaction
            }
            cassie_storage::adapter::error::TransactionErrorKind::InvalidSavepoint => {
                Self::InvalidSavepoint
            }
            cassie_storage::adapter::error::TransactionErrorKind::SerializationFailure => {
                Self::SerializationFailure
            }
            cassie_storage::adapter::error::TransactionErrorKind::DeadlockDetected => {
                Self::DeadlockDetected
            }
            cassie_storage::adapter::error::TransactionErrorKind::LockNotAvailable => {
                Self::LockNotAvailable
            }
        }
    }
}

impl From<cassie_storage::adapter::error::StorageRetryKind> for crate::app::StorageRetryKind {
    fn from(kind: cassie_storage::adapter::error::StorageRetryKind) -> Self {
        match kind {
            cassie_storage::adapter::error::StorageRetryKind::WriteConflict => Self::WriteConflict,
            cassie_storage::adapter::error::StorageRetryKind::WriteStall => Self::WriteStall,
            cassie_storage::adapter::error::StorageRetryKind::Fenced => Self::Fenced,
            cassie_storage::adapter::error::StorageRetryKind::Unavailable => Self::Unavailable,
        }
    }
}

impl crate::runtime::QueryExecutionControls {
    pub(crate) fn storage_request(
        &self,
    ) -> cassie_storage::adapter::StorageRequestContext<'_> {
        cassie_storage::adapter::StorageRequestContext::new(
            None,
            self.interruption(),
            self.request_memory(),
        )
        .with_scan_control(self.query_scan_control_scope())
    }
}

pub(crate) fn engine_storage_value_hooks() -> cassie_storage::adapter::StorageValueHooks {
    cassie_storage::adapter::StorageValueHooks::new(
        decode_storage_json,
        decode_storage_typed_value,
        validate_storage_graph_fields,
        crate::midge::scalar_expression::evaluate_stored_expression,
        crate::midge::scalar_expression::evaluate_stored_predicate,
    )
}

fn decode_storage_json(bytes: &[u8]) -> Result<serde_json::Value, cassie_storage::StorageError> {
    crate::midge::row_blob::decode_engine_json(bytes).map_err(|error| match error {
        crate::app::CassieError::Parse(message) => cassie_storage::StorageError::Parse(message),
        crate::app::CassieError::ResourceLimit(message) => {
            cassie_storage::StorageError::ResourceLimit(message)
        }
        crate::app::CassieError::QueryCanceled => cassie_storage::StorageError::QueryCanceled,
        crate::app::CassieError::DeadlineExceeded => {
            cassie_storage::StorageError::DeadlineExceeded
        }
        other => engine_callback_error(other),
    })
}


fn decode_storage_typed_value(
    value: &serde_json::Value,
    data_type: &cassie_core::types::DataType,
) -> Result<cassie_core::types::Value, cassie_storage::StorageError> {
    crate::executor::scan::try_json_to_typed_value(value, data_type).map_err(engine_callback_error)
}

fn validate_storage_graph_fields(
    schema: &cassie_core::types::Schema,
    document: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), cassie_storage::StorageError> {
    crate::executor::filter::graph_validation::validate_stored_fields(schema, document)
        .map_err(engine_callback_error)
}

fn engine_callback_error(error: crate::app::CassieError) -> cassie_storage::StorageError {
    cassie_storage::StorageError::EngineCallback(Box::new(error))
}

#[cfg(test)]
mod error_roundtrip_tests {
    use crate::app::CassieError;
    use cassie_storage::StorageError;

    #[test]
    fn should_restore_the_original_engine_error_from_a_storage_callback() {
        // Arrange
        let original = CassieError::Parse("bad stored JSON".to_owned());
        let callback_error = StorageError::EngineCallback(Box::new(original));

        // Act
        let restored = CassieError::from(callback_error);

        // Assert
        assert!(matches!(restored, CassieError::Parse(message) if message == "bad stored JSON"));
    }
}
