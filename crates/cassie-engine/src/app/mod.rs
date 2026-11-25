use std::cmp::Ordering as CmpOrdering;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use parking_lot::Mutex;
use serde::Serialize;
use uuid::Uuid;

use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};

use crate::catalog::{
    normalize_role_name, Catalog, CollectionSchema, ConstraintCheck, ConstraintOperator,
    FieldConstraint, RoleMeta,
};
use crate::config::{
    CassieRuntimeConfig, CohereRuntimeConfig, EmbeddingsRuntimeConfig, LocalRuntimeConfig,
    OpenAiCompatibleRuntimeConfig, OpenAiRuntimeConfig, SelfHostedEmbeddingRuntimeConfig,
    VoyageRuntimeConfig,
};
use crate::embeddings::{
    cohere::{CohereProvider, CohereProviderConfig},
    compatible::{OpenAiCompatibleProvider, OpenAiCompatibleProviderConfig},
    local::{LocalProvider, LocalProviderConfig},
    ollama::{OllamaProvider, OllamaProviderConfig},
    openai::{OpenAiProvider, OpenAiProviderConfig},
    tei::{TeiProvider, TeiProviderConfig},
    voyage::{VoyageProvider, VoyageProviderConfig},
    DistanceMetric, Embedding, EmbeddingError, NormalizedVectorRecord, VectorIndexRecord,
    VectorIndexType,
};
use crate::executor::{
    vector_prefilter_fallback_reason, vector_prefilter_supported, ColumnMeta, QueryError,
    QueryResult,
};
use crate::midge::adapter::{DocumentRef, Midge, MidgeScanTimings, RowDecode, RowFilter};
use crate::runtime::{
    query_cache, ExecutionMode, PlanCacheKey, QueryCancellationHandle, QueryExecutionControls,
    RuntimeFeedbackKey, RuntimeFeedbackObservation, RuntimeState,
};
use crate::sql::ast::{
    QueryStatement, TransactionAction, TransactionIsolation, TransactionStatement,
};
use crate::sql::binder;
use crate::types::{Value, Vector};

pub(crate) fn field_value_is_sql_null(
    value: Option<&serde_json::Value>,
    schema: Option<&CollectionSchema>,
    field: &str,
) -> bool {
    let Some(value) = value else {
        return true;
    };
    value.is_null()
        && !schema.is_some_and(|schema| {
            schema.fields.iter().any(|candidate| {
                candidate.name == field
                    && matches!(
                        candidate.data_type,
                        crate::types::DataType::Json | crate::types::DataType::Jsonb
                    )
            })
        })
}
use crate::vector::{
    cosine_distance_from_normalized_query, dot_distance_from_normalized_target,
    normalize as normalize_vector,
};

mod access;
#[doc(hidden)]
pub use access::relation_readable;
pub(crate) use access::{require_table_read, session_is_unprivileged};
mod cache;
mod copy_csv;
#[doc(hidden)]
pub mod copy_decode;
#[doc(hidden)]
pub mod enum_parameters;
mod error;
pub mod network_sessions;
mod session;
mod session_objects;
mod session_schema;
mod session_sequences;
mod session_settings;
mod state;
pub mod transport;
mod vector_backfill;
mod vector_write;
mod write_options;

use cache::{
    current_time_millis, NormalizedVectorCacheEntry, NormalizedVectorCacheKey, PlanCacheProvenance,
    QueryEmbeddingCacheKey, VectorSearchResultCacheKey,
};
#[doc(hidden)]
#[must_use]
pub fn unsupported_sql_error(sql: &str) -> Option<CassieError> {
    error::unsupported_sql_error(sql)
}
pub use error::{
    CassieError, CassieErrorDescriptor, CatalogObjectKind, DiagnosticError,
    OwnedNumericLiteralError, RetryableStorageError, StorageRetryKind,
};
pub(crate) use existing_constraint_checks::AddedColumnDefault;
pub use session::CassieSession;
pub(crate) use session::PrivateOutputs;
pub(crate) use session::{is_projection_write_mode, ProjectionCommitPolicy};
#[doc(hidden)]
pub use session::{BoundReadScope, BoundReadView, SessionTransactionStatus};
pub(crate) use session::{
    CommandCatalogScope, CopyBindingContext, SessionRandom, SessionReadScope, StagedWriteSnapshot,
    StatementDataRead, StatementMutationBatch, StatementReadScope, TransactionConflictIntent,
    TransactionRowChange,
};
#[doc(hidden)]
pub use session::{CopyCommandOwner, FailureAuthority, FailureAuthorityIdentity};
pub(crate) use session::{
    CursorOptions, SessionCursor, SessionPrepared, MAX_CURSORS_PER_SESSION,
    MAX_PREPARED_PER_SESSION,
};
pub(crate) use session_objects::parameter_type_display as prepared_parameter_type_display;
#[doc(hidden)]
pub use session_objects::SessionObjectError;
#[doc(hidden)]
pub use session_settings::{all as all_session_settings, SettingKind, SettingSpec};
pub use state::{Cassie, CassieRuntimeConfigState, RestTlsOptions};

mod auth;
pub(crate) use auth::AuthenticatedPrincipal;
#[doc(hidden)]
pub use auth::{NetworkPrincipal, NetworkRole};
mod auth_rate_limit;
mod bulk_ingest;
pub(crate) use bulk_ingest::CopyIngestOptions;
#[doc(hidden)]
pub use bulk_ingest::{copy_columns, CopyCsvOptions};
#[doc(hidden)]
pub use bulk_ingest::{CopyColumn, CopyIngestResult, CopySkippedRow};
#[doc(hidden)]
pub use copy_csv::parse_csv_payload as parse_copy_csv_payload_controlled;
mod consistency;
mod database_images;
mod databases;
mod defaults;
mod diagnostics;
mod document_scans;
pub(crate) use document_scans::{SessionRowCursor, StagedRowSet};
mod document_uniqueness;
mod documents;
mod embeddings;
mod existing_constraint_checks;
mod foreign_key_checks;
mod graph_schema;
mod hydration;
mod lifecycle;
mod operational;
mod portal_reads;
pub(crate) use portal_reads::PortalReadSpec;
mod check_numbers;
mod domain_enforcement;
mod domain_scope;
mod enum_checks;
mod enum_notice;
mod enum_scope;
mod integer_range;
mod publication_intent;
mod query;
mod query_bind;
mod query_bound;
mod query_cache_keys;
mod query_ctas;
mod query_describe;
mod query_explain;
mod query_feedback;
mod query_implicit_ddl;
mod query_lifecycle;
mod query_operation;
mod query_virtual_catalog;
mod query_wire;
pub(crate) use domain_enforcement::DomainRowRule;
mod type_scope;
pub(crate) use query_feedback::USER_SEARCH_PATH_ENTRY;
mod query_metrics;
mod query_parameters;
mod query_prepared;
mod query_transactions;
mod referential;
mod registry;
mod replay;
mod roles;
mod schema_cleanup;
mod snapshots;
mod statement_mutation;
mod transaction_error;
mod transaction_unique_waits;
#[cfg(test)]
pub(crate) use transaction_unique_waits::install_row_wait_observer;
pub(crate) use transaction_unique_waits::{
    collect_statement_unique_keys, collect_unique_keys, collect_upsert_unique_keys,
    has_unique_wait_owner, validate_unique_key_candidate, RowKeyClaim, RowTransactionKey,
    TransactionUniqueWaits, UniqueKeyClaim, UniqueTransactionKey,
};
mod storage_recovery;
mod transaction_semantics;
mod transaction_snapshot;
pub use transaction_error::TransactionErrorKind;
#[doc(hidden)]
pub use transaction_semantics::implicit_ddl_supported;
mod vector_helpers;
mod vector_search;
#[doc(hidden)]
pub use vector_search::VectorSearchInput;
mod write_refresh;

pub use consistency::ProjectionManifestExportOptions;
pub use database_images::{
    DatabaseBackupStream, DatabaseRestoreSession, DATABASE_IMAGE_VERSION,
    DATABASE_IMAGE_VERSION_WITH_PROTECTED_HEADER,
};
pub use query_explain::{QueryExplainOutput, QueryExplainPlan};
#[doc(hidden)]
pub use replay::{projection_concurrency_test_guard, set_projection_replay_prepare_barriers};
pub use replay::{ProjectionReplayBatch, ProjectionReplayEvent, ProjectionReplayReport};
pub use snapshots::{CassieSnapshotManifest, CassieSnapshotOptions};

#[cfg(test)]
mod commit_unknown_outcome_tests;
#[cfg(test)]
mod database_create_retry_restart_tests;
#[cfg(test)]
mod implicit_sequence_database_tests;
#[cfg(all(test, feature = "replay-benchmark-observation"))]
mod projection_sql_durability_tests;

#[cfg(test)]
mod projection_sql_mode_tests;
#[cfg(test)]
mod projection_sql_restart_tests;
#[cfg(test)]
mod query_failure_admission_tests;
#[cfg(test)]
mod query_failure_basic_tests;
#[cfg(test)]
mod query_failure_converted_tests;
#[cfg(test)]
mod query_failure_quarantine_tests;
#[cfg(test)]
mod query_failure_savepoint_tests;
#[cfg(any(test, feature = "transport-test-hooks"))]
#[doc(hidden)]
pub mod query_failure_watchdog;
#[cfg(test)]
mod query_result_cache_key_tests;
#[cfg(all(test, feature = "replay-benchmark-observation"))]
mod replay_api_durability_tests;
#[cfg(test)]
mod replay_maintenance_profile_tests;
#[cfg(test)]
mod statement_read_tests;
