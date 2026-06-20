use crate::app::Cassie;
use crate::app::CassieError;
use crate::embeddings::DistanceMetric;
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
pub struct SearchRequest {
    pub field: String,
    pub query: String,
    #[serde(default)]
    pub metric: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub offset: Option<usize>,
}

pub fn vector_search(cassie: &Cassie, collection: &str, body: &[u8]) -> Result<Value, CassieError> {
    let request: SearchRequest =
        serde_json::from_slice(body).map_err(|error| CassieError::Parse(error.to_string()))?;

    let metric = request
        .metric
        .as_deref()
        .and_then(|metric| metric.parse::<DistanceMetric>().ok());
    let requested_metric = request.metric.as_deref();

    if let Some(requested_metric) = requested_metric {
        if metric.is_none() {
            return Err(CassieError::InvalidEmbedding(format!(
                "unsupported metric '{}'. expected cosine/l2/dot",
                requested_metric
            )));
        }
    }

    let limit = request.limit.unwrap_or(10);
    let offset = request.offset.unwrap_or(0);

    let result = cassie.execute_vector_search(
        collection,
        &request.field,
        &request.query,
        metric,
        limit,
        offset,
    )?;

    Ok(serde_json::to_value(result)
        .unwrap_or_else(|_| serde_json::json!({"error":"invalid result"})))
}
