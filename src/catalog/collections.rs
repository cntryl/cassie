use serde::{Deserialize, Serialize};

pub fn is_reserved_namespace(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "information_schema" | "pg_catalog" | "public"
    )
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionMeta {
    pub name: String,
    pub description: Option<String>,
    #[serde(default)]
    pub storage_mode: CollectionStorageMode,
    #[serde(default = "default_collection_storage_version")]
    pub storage_version: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollectionStorageMode {
    RowStore,
    ColumnIndexed,
    ColumnStore,
}

impl Default for CollectionStorageMode {
    fn default() -> Self {
        Self::RowStore
    }
}

impl CollectionStorageMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::RowStore => "row-store",
            Self::ColumnIndexed => "column-indexed",
            Self::ColumnStore => "column-store",
        }
    }

    pub fn parse_option(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "row_store" | "row-store" => Some(Self::RowStore),
            "column_indexed" | "column-indexed" => Some(Self::ColumnIndexed),
            "column_store" | "column-store" => Some(Self::ColumnStore),
            _ => None,
        }
    }

    pub fn uses_column_store_storage(self) -> bool {
        matches!(self, Self::ColumnStore)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionRebuildState {
    Idle,
    Rebuilding,
    Failed,
}

impl ProjectionRebuildState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Rebuilding => "rebuilding",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionFreshness {
    Unknown,
    Fresh,
    Stale,
    Rebuilding,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionVerificationState {
    Unknown,
    Current,
    Stale,
    Missing,
    Incomplete,
    Incompatible,
    Empty,
    Pending,
    Running,
    Verified,
    Failed,
    Unverifiable,
    Skipped,
}

impl Default for ProjectionVerificationState {
    fn default() -> Self {
        Self::Unknown
    }
}

impl ProjectionVerificationState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Current => "current",
            Self::Stale => "stale",
            Self::Missing => "missing",
            Self::Incomplete => "incomplete",
            Self::Incompatible => "incompatible",
            Self::Empty => "empty",
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Verified => "verified",
            Self::Failed => "failed",
            Self::Unverifiable => "unverifiable",
            Self::Skipped => "skipped",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectionHashAlgorithmMeta {
    pub algorithm: String,
    pub digest_length: u16,
    pub canonical_encoder_version: u16,
    pub hash_version: u16,
}

impl Default for ProjectionHashAlgorithmMeta {
    fn default() -> Self {
        Self {
            algorithm: "cassie-fnv128".to_string(),
            digest_length: 16,
            canonical_encoder_version: 1,
            hash_version: 1,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectionHashCoverageMeta {
    pub state: ProjectionVerificationState,
    pub row_count: u64,
    pub range_count: u64,
    pub source_checkpoint: Option<String>,
    pub projection_version_id: Option<String>,
    pub last_computed_ms: Option<u64>,
    pub digest: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectionHashMeta {
    #[serde(default)]
    pub algorithm: ProjectionHashAlgorithmMeta,
    #[serde(default)]
    pub rows: ProjectionHashCoverageMeta,
    #[serde(default)]
    pub ranges: ProjectionHashCoverageMeta,
    #[serde(default)]
    pub root: ProjectionHashCoverageMeta,
}

impl Default for ProjectionHashMeta {
    fn default() -> Self {
        Self {
            algorithm: ProjectionHashAlgorithmMeta::default(),
            rows: ProjectionHashCoverageMeta::default(),
            ranges: ProjectionHashCoverageMeta::default(),
            root: ProjectionHashCoverageMeta::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectionRebuildVerificationMeta {
    pub state: ProjectionVerificationState,
    pub started_ms: Option<u64>,
    pub completed_ms: Option<u64>,
    pub mismatch_count: u64,
    pub unverifiable_ranges: u64,
    pub failure_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectionIntegrityReportMeta {
    pub state: ProjectionVerificationState,
    pub target: Option<String>,
    pub version_id: Option<String>,
    pub mode: String,
    pub checked_components: Vec<String>,
    pub skipped_components: Vec<String>,
    pub mismatch_count: u64,
    pub missing_count: u64,
    pub stale_count: u64,
    pub repairable: bool,
    pub elapsed_ms: u64,
    pub completed_ms: Option<u64>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectionComparisonReportMeta {
    pub report_id: String,
    pub created_ms: u64,
    pub target: String,
    pub target_version_id: Option<String>,
    pub state: String,
    pub compatibility_status: String,
    pub root_digest: Option<String>,
    pub manifest_digest: Option<String>,
    pub mismatch_count: u64,
    pub unverifiable_count: u64,
    pub diagnostic_sample: Vec<String>,
    pub last_error: Option<String>,
}

impl Default for ProjectionFreshness {
    fn default() -> Self {
        Self::Unknown
    }
}

impl ProjectionFreshness {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Fresh => "fresh",
            Self::Stale => "stale",
            Self::Rebuilding => "rebuilding",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionKind {
    Collection,
    Materialized,
}

impl Default for ProjectionKind {
    fn default() -> Self {
        Self::Collection
    }
}

impl ProjectionKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Collection => "collection",
            Self::Materialized => "materialized",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaterializedProjectionState {
    Building,
    Ready,
    Stale,
    Failed,
}

impl MaterializedProjectionState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Building => "building",
            Self::Ready => "ready",
            Self::Stale => "stale",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionVersionState {
    Building,
    Built,
    Active,
    Failed,
    Retired,
}

impl ProjectionVersionState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Building => "building",
            Self::Built => "built",
            Self::Active => "active",
            Self::Failed => "failed",
            Self::Retired => "retired",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaterializedProjectionMeta {
    pub name: String,
    pub query: String,
    #[serde(default)]
    pub options: std::collections::BTreeMap<String, String>,
    pub output_collection: String,
    pub source_collections: Vec<String>,
    pub schema_epoch: u64,
    pub output_schema: crate::types::Schema,
    pub state: MaterializedProjectionState,
    pub definition_fingerprint: u64,
    pub refresh_cursor: Option<String>,
    pub last_built_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectionVersionMeta {
    pub version_id: String,
    pub output_collection: String,
    pub definition_fingerprint: u64,
    pub source_schema_epoch: u64,
    pub state: ProjectionVersionState,
    pub created_ms: u64,
    pub activated_ms: Option<u64>,
    pub retired_ms: Option<u64>,
    pub last_error: Option<String>,
    #[serde(default)]
    pub verification: ProjectionRebuildVerificationMeta,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectionSwapMeta {
    pub target_version_id: Option<String>,
    pub previous_version_id: Option<String>,
    pub swapped_at_ms: Option<u64>,
    pub unsafe_override: bool,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectionMeta {
    #[serde(default)]
    pub projection_id: String,
    pub collection: String,
    #[serde(default)]
    pub kind: ProjectionKind,
    pub schema_version: u32,
    pub offset: u64,
    pub lag: u64,
    pub rebuild_state: ProjectionRebuildState,
    #[serde(default)]
    pub source_identity: Option<String>,
    #[serde(default)]
    pub source_checkpoint: Option<String>,
    #[serde(default)]
    pub source_position: Option<u64>,
    #[serde(default)]
    pub last_applied_event_id: Option<String>,
    #[serde(default)]
    pub replay_batch_id: Option<String>,
    #[serde(default)]
    pub applied_event_count: u64,
    #[serde(default)]
    pub skipped_duplicate_count: u64,
    #[serde(default)]
    pub freshness: ProjectionFreshness,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default)]
    pub materialized: Option<MaterializedProjectionMeta>,
    #[serde(default)]
    pub versions: Vec<ProjectionVersionMeta>,
    #[serde(default)]
    pub active_version: Option<String>,
    #[serde(default)]
    pub swap: ProjectionSwapMeta,
    #[serde(default)]
    pub hashes: ProjectionHashMeta,
    #[serde(default)]
    pub verification: ProjectionRebuildVerificationMeta,
    #[serde(default)]
    pub integrity: ProjectionIntegrityReportMeta,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamespaceMeta {
    pub name: String,
    pub description: Option<String>,
}

impl CollectionMeta {
    pub fn new(name: impl Into<String>, description: Option<String>) -> Self {
        Self::new_with_storage_mode(name, description, CollectionStorageMode::RowStore)
    }

    pub fn new_with_storage_mode(
        name: impl Into<String>,
        description: Option<String>,
        storage_mode: CollectionStorageMode,
    ) -> Self {
        Self {
            name: name.into(),
            description,
            storage_mode,
            storage_version: default_collection_storage_version(),
        }
    }
}

fn default_collection_storage_version() -> u16 {
    1
}

impl ProjectionMeta {
    pub fn new(collection: impl Into<String>, schema_version: u32) -> Self {
        let collection = collection.into();
        Self {
            projection_id: collection.clone(),
            collection,
            kind: ProjectionKind::Collection,
            schema_version,
            offset: 0,
            lag: 0,
            rebuild_state: ProjectionRebuildState::Idle,
            source_identity: None,
            source_checkpoint: None,
            source_position: None,
            last_applied_event_id: None,
            replay_batch_id: None,
            applied_event_count: 0,
            skipped_duplicate_count: 0,
            freshness: ProjectionFreshness::Unknown,
            last_error: None,
            materialized: None,
            versions: Vec::new(),
            active_version: None,
            swap: ProjectionSwapMeta::default(),
            hashes: ProjectionHashMeta::default(),
            verification: ProjectionRebuildVerificationMeta::default(),
            integrity: ProjectionIntegrityReportMeta::default(),
        }
    }

    pub fn materialized(
        name: impl Into<String>,
        query: impl Into<String>,
        source_collections: Vec<String>,
        output_schema: crate::types::Schema,
        options: std::collections::BTreeMap<String, String>,
        schema_epoch: u64,
        definition_fingerprint: u64,
        created_ms: u64,
    ) -> Self {
        let name = name.into();
        let query = query.into();
        let version_id = "v1".to_string();
        let output_collection = materialized_output_collection(&name, &version_id);
        Self {
            projection_id: name.clone(),
            collection: name.clone(),
            kind: ProjectionKind::Materialized,
            schema_version: 1,
            offset: 0,
            lag: 0,
            rebuild_state: ProjectionRebuildState::Rebuilding,
            source_identity: source_collections.first().cloned(),
            source_checkpoint: None,
            source_position: None,
            last_applied_event_id: None,
            replay_batch_id: None,
            applied_event_count: 0,
            skipped_duplicate_count: 0,
            freshness: ProjectionFreshness::Rebuilding,
            last_error: None,
            materialized: Some(MaterializedProjectionMeta {
                name: name.clone(),
                query,
                options,
                output_collection: output_collection.clone(),
                source_collections,
                schema_epoch,
                output_schema,
                state: MaterializedProjectionState::Building,
                definition_fingerprint,
                refresh_cursor: None,
                last_built_ms: None,
            }),
            versions: vec![ProjectionVersionMeta {
                version_id: version_id.clone(),
                output_collection,
                definition_fingerprint,
                source_schema_epoch: schema_epoch,
                state: ProjectionVersionState::Building,
                created_ms,
                activated_ms: None,
                retired_ms: None,
                last_error: None,
                verification: ProjectionRebuildVerificationMeta::default(),
            }],
            active_version: None,
            swap: ProjectionSwapMeta::default(),
            hashes: ProjectionHashMeta::default(),
            verification: ProjectionRebuildVerificationMeta::default(),
            integrity: ProjectionIntegrityReportMeta::default(),
        }
    }

    pub fn projection_id(&self) -> &str {
        if self.projection_id.is_empty() {
            &self.collection
        } else {
            &self.projection_id
        }
    }

    pub fn active_output_collection(&self) -> Option<&str> {
        let active = self.active_version.as_ref()?;
        self.versions
            .iter()
            .find(|version| &version.version_id == active)
            .map(|version| version.output_collection.as_str())
    }
}

pub fn materialized_output_collection(name: &str, version_id: &str) -> String {
    format!("__cassie_projection_{name}_{version_id}")
}

impl NamespaceMeta {
    pub fn new(name: impl Into<String>, description: Option<String>) -> Self {
        Self {
            name: name.into(),
            description,
        }
    }
}
