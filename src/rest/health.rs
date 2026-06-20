use crate::app::Cassie;

pub fn health(cassie: &Cassie) -> serde_json::Value {
    cassie.health()
}

pub fn metrics(cassie: &Cassie) -> serde_json::Value {
    cassie.metrics()
}

pub fn liveness(_cassie: &Cassie) -> serde_json::Value {
    serde_json::json!({"ready": true})
}
