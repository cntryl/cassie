//! The version 1 JSON export of a path. JSON is an export, not the native
//! identity or equality mechanism: every key value is a JSON string, so a
//! `bigint` such as 9007199254740993 never rounds through a JSON number.
use serde_json::{json, Map, Value as Json};

use super::model::{ElementRef, Path};

fn element(reference: &ElementRef) -> Json {
    let key_parts = reference
        .key
        .iter()
        .map(|part| json!({ "type": part.type_name(), "value": part.value_text() }))
        .collect::<Vec<_>>();
    let mut object = Map::new();
    object.insert("database_id".into(), json!(reference.database_id.to_string()));
    object.insert("graph_id".into(), json!(reference.graph_id.to_string()));
    object.insert("mapping_id".into(), json!(reference.mapping_id.to_string()));
    object.insert("kind".into(), json!(reference.kind.name()));
    object.insert("key_parts".into(), Json::Array(key_parts));
    Json::Object(object)
}

/// The exact `version`, `vertices`, `edges` and `directions` document.
#[must_use]
pub fn path_to_json(path: &Path) -> Json {
    json!({
        "version": 1,
        "vertices": path.vertices.iter().map(element).collect::<Vec<_>>(),
        "edges": path.edges.iter().map(element).collect::<Vec<_>>(),
        "directions": path.directions.iter().map(|direction| direction.tag()).collect::<Vec<_>>(),
    })
}
