use std::cmp::Ordering as CmpOrdering;
use std::collections::{BTreeMap, BinaryHeap};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use parking_lot::Mutex;
use serde::Serialize;
use uuid::Uuid;

use argon2::{
    password_hash::{rand_core::OsRng, SaltString},
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
};

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
    query_cache, ExecutionMode, PlanCacheKey, QueryExecutionControls, RuntimeFeedbackKey,
    RuntimeFeedbackObservation, RuntimeState,
};
use crate::sql::ast::{
    QueryStatement, TransactionAction, TransactionIsolation, TransactionStatement,
};
use crate::sql::{binder, parser};
use crate::types::{Value, Vector};
use crate::vector::{
    cosine_distance_from_normalized_query, dot_distance_from_normalized_target,
    normalize as normalize_vector,
};

mod access;
mod cache;
mod error;
mod session;
mod state;

use cache::{
    current_time_millis, NormalizedVectorCacheEntry, NormalizedVectorCacheKey, PlanCacheProvenance,
    QueryEmbeddingCacheKey, VectorSearchResultCacheKey,
};
pub(crate) use error::unsupported_sql_error;
pub use error::{CassieError, CatalogObjectKind};
pub use session::CassieSession;
pub(crate) use session::TransactionRowChange;
pub use state::{Cassie, CassieRuntimeConfigState};

mod auth;
mod bulk_ingest;
mod consistency;
mod defaults;
mod diagnostics;
mod document_scans;
mod documents;
mod embeddings;
mod hydration;
mod lifecycle;
mod operational;
mod query;
mod query_explain;
mod query_feedback;
mod query_prepared;
mod query_transactions;
mod registry;
mod replay;
mod roles;
mod schema_cleanup;
mod snapshots;
mod vector_helpers;
mod vector_search;
mod write_refresh;

pub use consistency::ProjectionManifestExportOptions;
pub use query_explain::{QueryExplainOutput, QueryExplainPlan};
pub use replay::{ProjectionReplayBatch, ProjectionReplayEvent, ProjectionReplayReport};
pub use snapshots::{CassieSnapshotManifest, CassieSnapshotOptions};
