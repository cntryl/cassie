use crate::app::Cassie;

#[must_use]
pub fn health(cassie: &Cassie) -> serde_json::Value {
    cassie.health()
}

#[must_use]
pub fn metrics(cassie: &Cassie) -> serde_json::Value {
    cassie.metrics()
}

#[must_use]
pub fn liveness(_cassie: &Cassie) -> serde_json::Value {
    serde_json::json!({"ready": true})
}

#[must_use]
pub fn targetz(_cassie: &Cassie) -> serde_json::Value {
    serde_json::json!({"ready": true})
}
