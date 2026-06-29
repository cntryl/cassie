use super::{CmpOrdering, DistanceMetric, CollectionSchema, ColumnMeta, DocumentRef, Value, Vector};

#[derive(Debug, Clone, PartialEq)]
pub(super) struct ScoredVectorCandidate {
    pub(super) distance: f64,
    pub(super) id: String,
}

impl ScoredVectorCandidate {
    pub(super) fn is_better_than(&self, other: &Self) -> bool {
        compare_scored_vector_candidates(self, other) == CmpOrdering::Less
    }
}

impl Eq for ScoredVectorCandidate {}

impl PartialOrd for ScoredVectorCandidate {
    fn partial_cmp(&self, other: &Self) -> Option<CmpOrdering> {
        Some(self.cmp(other))
    }
}

impl Ord for ScoredVectorCandidate {
    fn cmp(&self, other: &Self) -> CmpOrdering {
        compare_scored_vector_candidates(self, other)
    }
}

pub(super) fn compare_scored_vector_candidates(
    left: &ScoredVectorCandidate,
    right: &ScoredVectorCandidate,
) -> CmpOrdering {
    left.distance
        .total_cmp(&right.distance)
        .then_with(|| left.id.cmp(&right.id))
}

pub(super) fn vector_distance_for_metric(
    metric: &DistanceMetric,
    query: &[f32],
    target: &[f32],
) -> f64 {
    if query.is_empty() || target.is_empty() || query.len() != target.len() {
        return f64::INFINITY;
    }

    match metric {
        DistanceMetric::Cosine => crate::vector::cosine_distance(query, target),
        DistanceMetric::L2 => crate::vector::l2_distance(query, target),
        DistanceMetric::Dot => crate::vector::dot_distance(query, target),
    }
}

pub(super) fn vector_from_json(value: &serde_json::Value) -> Option<Vec<f32>> {
    let values = value.as_array()?;
    let mut vector = Vec::with_capacity(values.len());
    for value in values {
        vector.push(value.as_f64()? as f32);
    }
    Some(vector)
}

pub(super) fn vector_search_columns(schema: &CollectionSchema) -> Vec<ColumnMeta> {
    let mut columns = Vec::with_capacity(schema.fields.len() + 1);
    columns.push(ColumnMeta::from_data_type(
        "id".to_string(),
        crate::types::DataType::Text,
    ));
    for field in &schema.fields {
        if field.name != "id" {
            columns.push(ColumnMeta::from_data_type(
                field.name.clone(),
                field.data_type.clone(),
            ));
        }
    }
    columns
}

pub(super) fn vector_search_row(schema: &CollectionSchema, document: DocumentRef) -> Vec<Value> {
    let mut row = Vec::with_capacity(schema.fields.len() + 1);
    row.push(Value::String(document.id));
    for field in &schema.fields {
        if field.name == "id" {
            continue;
        }
        let value = document
            .payload
            .get(&field.name)
            .map_or(Value::Null, |value| json_to_query_value(value, &field.data_type));
        row.push(value);
    }
    row
}

pub(super) fn json_to_query_value(
    value: &serde_json::Value,
    data_type: &crate::types::DataType,
) -> Value {
    if value.is_null() {
        return Value::Null;
    }
    if matches!(data_type, crate::types::DataType::Vector(_)) {
        return vector_from_json(value)
            .map_or(Value::Null, |vector| Value::Vector(Vector::new(vector)));
    }
    if let Some(value) = value.as_str() {
        return Value::String(value.to_string());
    }
    if let Some(value) = value.as_bool() {
        return Value::Bool(value);
    }
    if let Some(value) = value.as_i64() {
        return Value::Int64(value);
    }
    if let Some(value) = value.as_u64().and_then(|value| i64::try_from(value).ok()) {
        return Value::Int64(value);
    }
    if let Some(value) = value.as_f64() {
        return Value::Float64(value);
    }
    Value::Json(value.clone())
}

pub(super) fn project_payload_fields(
    payload: &serde_json::Value,
    fields: &[String],
) -> serde_json::Value {
    let Some(object) = payload.as_object() else {
        return serde_json::Value::Object(serde_json::Map::new());
    };

    let mut projected = serde_json::Map::new();
    for field in fields {
        if let Some((_, value)) = object
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(field))
        {
            projected.insert(field.clone(), value.clone());
        }
    }

    serde_json::Value::Object(projected)
}
