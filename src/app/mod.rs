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
use crate::sql::{binder, parser};
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
mod cache;
mod error;
mod session;
mod session_objects;
mod session_schema;
mod session_sequences;
pub(crate) use session_sequences::SequenceRestorer;
mod session_settings;
mod state;
mod vector_backfill;
mod vector_write;
mod write_options;

use cache::{
    current_time_millis, NormalizedVectorCacheEntry, NormalizedVectorCacheKey, PlanCacheProvenance,
    QueryEmbeddingCacheKey, VectorSearchResultCacheKey,
};
pub(crate) use error::unsupported_sql_error;
pub use error::{
    CassieError, CatalogObjectKind, DiagnosticError, RetryableStorageError, StorageRetryKind,
};
pub use session::CassieSession;
pub(crate) use session::{
    CursorOptions, SessionCursor, SessionPrepared, MAX_CURSORS_PER_SESSION,
    MAX_PREPARED_PER_SESSION,
};
pub(crate) use session::{
    SessionRandom, SessionReadScope, StagedWriteSnapshot, StatementMutationBatch, StatementOverlay,
    TransactionConflictIntent, TransactionRowChange,
};
pub(crate) use session_objects::parameter_type_display as prepared_parameter_type_display;
pub(crate) use session_objects::SessionObjectError;
pub(crate) use session_settings::all as all_session_settings;
pub(crate) use session_settings::SettingKind;
pub use state::{Cassie, CassieRuntimeConfigState};

mod auth;
pub(crate) use auth::AuthenticatedPrincipal;
mod auth_rate_limit;
mod bulk_ingest;
pub(crate) use bulk_ingest::{
    copy_columns, parse_copy_csv_payload_with_force, CopyColumn, CopyCsvOptions, CopyIngestOptions,
};
mod consistency;
mod database_images;
mod databases;
mod defaults;
mod diagnostics;
mod document_scans;
pub(crate) use document_scans::SessionRowCursor;
mod document_uniqueness;
mod documents;
mod embeddings;
mod existing_constraint_checks;
mod foreign_key_checks;
mod hydration;
mod lifecycle;
mod operational;
mod portal_reads;
pub(crate) use portal_reads::PortalReadSpec;
mod query;
mod query_bind;
mod query_bound;
mod query_cache_keys;
mod query_ctas;
mod query_describe;
mod query_explain;
mod query_feedback;
mod query_lifecycle;
mod query_wire;
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
pub(crate) use transaction_unique_waits::{
    collect_unique_keys, collect_upsert_unique_keys, validate_unique_key_candidate,
    TransactionUniqueWaits, UniqueKeyClaim, UniqueTransactionKey,
};
mod transaction_semantics;
pub use transaction_error::TransactionErrorKind;
pub(crate) use transaction_semantics::implicit_ddl_supported;
mod vector_helpers;
mod vector_search;
pub(crate) use vector_search::VectorSearchInput;
mod write_refresh;

pub use consistency::ProjectionManifestExportOptions;
pub use database_images::{DatabaseBackupStream, DatabaseRestoreSession, DATABASE_IMAGE_VERSION};
pub use query_explain::{QueryExplainOutput, QueryExplainPlan};
#[doc(hidden)]
pub use replay::{projection_concurrency_test_guard, set_projection_replay_prepare_barriers};
pub use replay::{ProjectionReplayBatch, ProjectionReplayEvent, ProjectionReplayReport};
pub use snapshots::{CassieSnapshotManifest, CassieSnapshotOptions};

#[cfg(test)]
mod query_result_cache_key_tests;
#[cfg(test)]
mod statement_read_tests;
