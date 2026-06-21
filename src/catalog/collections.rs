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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionRebuildState {
    Idle,
    Rebuilding,
    Failed,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamespaceMeta {
    pub name: String,
    pub description: Option<String>,
}

impl CollectionMeta {
    pub fn new(name: impl Into<String>, description: Option<String>) -> Self {
        Self {
            name: name.into(),
            description,
        }
    }
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
        }
    }

    pub fn materialized(
        name: impl Into<String>,
        query: impl Into<String>,
        source_collections: Vec<String>,
        output_schema: crate::types::Schema,
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
            }],
            active_version: None,
            swap: ProjectionSwapMeta::default(),
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
