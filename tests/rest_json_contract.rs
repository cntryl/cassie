use cassie::app::Cassie;
use cassie::rest::{collections, documents, health, query};
use uuid::Uuid;

fn with_fallback() {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
}

fn data_dir(label: &str) -> String {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "cassie-rest-json-contract-{label}-{}",
        Uuid::new_v4()
    ));
    path.to_string_lossy().to_string()
}

fn setup_query_values(cassie: &Cassie) {
    let session = cassie.create_session("postgres", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE rest_json_query_values (
                text_value TEXT,
                int_value INT,
                float_value FLOAT,
                bool_value BOOLEAN,
                null_value TEXT,
                embedding VECTOR(2),
                payload JSON
            )",
            Vec::new(),
        )
        .expect("create values table");
    cassie
        .ingest_document(
            "rest_json_query_values",
            serde_json::json!({
                "text_value": "alpha",
                "int_value": 7,
                "float_value": 3.5,
                "bool_value": true,
                "null_value": null,
                "embedding": [1.0, 2.5],
                "payload": {
                    "MixedCase": {
                        "innerKey": [1, true, null]
                    }
                }
            }),
        )
        .expect("ingest values row");
}

fn assert_no_uppercase_response_keys(value: &serde_json::Value, skipped_prefixes: &[&str]) {
    assert_no_uppercase_response_keys_at(value, "$", skipped_prefixes);
}

fn assert_no_uppercase_response_keys_at(
    value: &serde_json::Value,
    path: &str,
    skipped_prefixes: &[&str],
) {
    if skipped_prefixes
        .iter()
        .any(|skipped_prefix| path.starts_with(skipped_prefix))
    {
        return;
    }

    match value {
        serde_json::Value::Object(object) => {
            for (key, nested) in object {
                assert!(
                    key.chars().all(|character| !character.is_ascii_uppercase()),
                    "response key '{key}' at {path} contains uppercase characters"
                );
                assert_no_uppercase_response_keys_at(
                    nested,
                    format!("{path}.{key}").as_str(),
                    skipped_prefixes,
                );
            }
        }
        serde_json::Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                assert_no_uppercase_response_keys_at(
                    item,
                    format!("{path}[{index}]").as_str(),
                    skipped_prefixes,
                );
            }
        }
        serde_json::Value::Null
        | serde_json::Value::Bool(_)
        | serde_json::Value::Number(_)
        | serde_json::Value::String(_) => {}
    }
}

fn is_snake_case_property(name: &str) -> bool {
    !name.is_empty()
        && name.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
        })
}

fn line_indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

fn yaml_key(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('-') {
        return None;
    }
    let (key, _) = trimmed.split_once(':')?;
    Some(key.trim_matches('\'').trim_matches('"'))
}

#[test]
fn should_serialize_admin_query_values_as_plain_json() {
    // Arrange
    with_fallback();
    let path = data_dir("query-values");
    let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
    cassie.startup().expect("startup");
    setup_query_values(&cassie);

    // Act
    let result = query::execute(
        &cassie,
        "postgres",
        serde_json::json!({
            "sql": "SELECT text_value, int_value, float_value, bool_value, null_value, embedding, payload FROM rest_json_query_values"
        })
        .to_string()
        .as_bytes(),
    )
    .expect("execute query");
    let payload = serde_json::to_value(result).expect("query result json");

    // Assert
    assert_eq!(payload["command"], "SELECT");
    assert_eq!(
        payload["rows"][0],
        serde_json::json!([
            "alpha",
            7,
            3.5,
            true,
            null,
            [1.0, 2.5],
            {
                "MixedCase": {
                    "innerKey": [1, true, null]
                }
            }
        ])
    );
    assert!(payload["rows"][0][0].get("String").is_none());
    assert!(payload["rows"][0][1].get("Int64").is_none());
    assert!(payload["rows"][0][2].get("Float64").is_none());

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_preserve_user_payload_key_shape() {
    // Arrange
    with_fallback();
    let path = data_dir("payload-shape");
    let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
    cassie.startup().expect("startup");
    let collection = "rest_json_payload_shape";
    collections::create(
        &cassie,
        serde_json::json!({
            "name": collection,
            "fields": [
                {"name": "CamelCase", "type": "text"},
                {"name": "title", "type": "text"},
                {"name": "payload", "type": "json"}
            ]
        })
        .to_string()
        .as_bytes(),
    )
    .expect("create collection");

    // Act
    let created = documents::create(
        &cassie,
        collection,
        serde_json::json!({
            "CamelCase": "kept",
            "title": "alpha",
            "payload": {
                "MixedKey": {
                    "innerValue": 1
                }
            }
        })
        .to_string()
        .as_bytes(),
    )
    .expect("create document");
    let id = created["id"].as_str().expect("document id");
    let loaded = documents::get(&cassie, collection, id).expect("get document");

    // Assert
    assert_eq!(loaded["CamelCase"], "kept");
    assert_eq!(loaded["payload"]["MixedKey"]["innerValue"], 1);

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_keep_metadata_response_keys_snake_case() {
    // Arrange
    with_fallback();
    let path = data_dir("response-keys");
    let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
    cassie.startup().expect("startup");
    setup_query_values(&cassie);
    let collection_response = collections::create(
        &cassie,
        serde_json::json!({
            "name": "rest_json_contract_docs",
            "fields": [{"name": "title", "type": "text"}]
        })
        .to_string()
        .as_bytes(),
    )
    .expect("create collection");

    // Act
    let responses = vec![
        health::liveness(&cassie),
        health::health(&cassie),
        collection_response,
        serde_json::to_value(query::schema(&cassie)).expect("schema json"),
        serde_json::to_value(
            query::validate(
                &cassie,
                serde_json::json!({"sql": "SELECT text_value FROM rest_json_query_values"})
                    .to_string()
                    .as_bytes(),
            )
            .expect("validate query"),
        )
        .expect("validate json"),
        serde_json::to_value(
            query::execute(
                &cassie,
                "postgres",
                serde_json::json!({"sql": "SELECT payload FROM rest_json_query_values"})
                    .to_string()
                    .as_bytes(),
            )
            .expect("execute query"),
        )
        .expect("execute json"),
    ];

    // Assert
    for response in responses {
        assert_no_uppercase_response_keys(&response, &["$.rows"]);
    }

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_keep_openapi_payload_properties_snake_case() {
    // Arrange
    let openapi = std::fs::read_to_string("public/openapi.yml").expect("openapi");
    let mut properties_indent = None;
    let openapi_lines = openapi.lines().enumerate();

    // Act
    for (line_index, line) in openapi_lines {
        let indent = line_indent(line);
        if properties_indent.is_some_and(|active_indent| indent <= active_indent) {
            properties_indent = None;
        }

        if line.trim() == "properties:" {
            properties_indent = Some(indent);
            continue;
        }

        let Some(active_indent) = properties_indent else {
            continue;
        };
        if indent != active_indent + 2 {
            continue;
        }
        let Some(property) = yaml_key(line) else {
            continue;
        };

        // Assert
        assert!(
            is_snake_case_property(property),
            "OpenAPI payload property '{property}' on line {} is not snake_case",
            line_index + 1
        );
    }
}
