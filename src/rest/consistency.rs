use crate::app::{Cassie, CassieError, ProjectionManifestExportOptions};
use crate::catalog::ProjectionVerificationManifest;

#[derive(serde::Deserialize)]
#[serde(rename_all = "snake_case")]
struct ExportManifestRequest {
    #[serde(default)]
    instance_id: Option<String>,
    #[serde(default)]
    generated_ms: Option<u64>,
    #[serde(default)]
    ttl_ms: Option<u64>,
    #[serde(default)]
    include_row_hashes: bool,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "snake_case")]
struct ConsistencyCheckRequest {
    manifests: Vec<ProjectionVerificationManifest>,
}

/// # Errors
///
/// Returns an error when validation, storage, or execution fails.
pub fn export_manifest(
    cassie: &Cassie,
    projection: &str,
    body: &[u8],
) -> Result<serde_json::Value, CassieError> {
    let request = if body.is_empty() {
        ExportManifestRequest {
            instance_id: None,
            generated_ms: None,
            ttl_ms: None,
            include_row_hashes: false,
        }
    } else {
        serde_json::from_slice(body).map_err(|error| CassieError::Parse(error.to_string()))?
    };
    let mut options = ProjectionManifestExportOptions::for_instance(
        request.instance_id.unwrap_or_else(|| "local".to_string()),
    );
    options.generated_ms = request.generated_ms;
    options.ttl_ms = request.ttl_ms;
    options.include_row_hashes = request.include_row_hashes;
    serde_json::to_value(cassie.export_projection_verification_manifest(projection, options)?)
        .map_err(|error| CassieError::Parse(error.to_string()))
}

/// # Errors
///
/// Returns an error when validation, storage, or execution fails.
pub fn compare_manifests(cassie: &Cassie, body: &[u8]) -> Result<serde_json::Value, CassieError> {
    let request: ConsistencyCheckRequest =
        serde_json::from_slice(body).map_err(|error| CassieError::Parse(error.to_string()))?;
    serde_json::to_value(cassie.compare_projection_verification_manifests(request.manifests)?)
        .map_err(|error| CassieError::Parse(error.to_string()))
}

#[must_use]
pub fn reports(cassie: &Cassie) -> serde_json::Value {
    serde_json::json!({
        "reports": cassie.catalog.latest_projection_consistency_reports(),
    })
}
