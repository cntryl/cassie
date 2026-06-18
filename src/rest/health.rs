use crate::app::Cassie;

pub async fn health(cassie: &Cassie) -> serde_json::Value {
    cassie.health().await
}

pub async fn metrics(cassie: &Cassie) -> serde_json::Value {
    cassie.metrics().await
}

pub async fn liveness(_cassie: &Cassie) -> serde_json::Value {
    serde_json::json!({"ready": true})
}
