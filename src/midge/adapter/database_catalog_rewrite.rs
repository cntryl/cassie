use super::CassieError;

const DATABASE_NAME_COMPONENT_INDEX: usize = 3;

pub(super) fn key_components(key: &[u8]) -> impl Iterator<Item = &[u8]> {
    key.split(|byte| *byte == cntryl_lexkey::LexKey::SEPARATOR)
}

fn database_name_component(key: &[u8]) -> Option<&[u8]> {
    key_components(key).nth(DATABASE_NAME_COMPONENT_INDEX)
}

pub(super) fn key_family(key: &[u8]) -> Option<&str> {
    key_components(key)
        .nth(2)
        .and_then(|component| std::str::from_utf8(component).ok())
}

pub(super) fn catalog_entry_belongs_to_database(key: &[u8], value: &[u8], database: &str) -> bool {
    let Some(family) = key_family(key) else {
        return false;
    };
    if matches!(family, "collections" | "namespaces") {
        return serde_json::from_slice::<Vec<String>>(value).is_ok_and(|values| {
            values
                .iter()
                .any(|value| catalog_name_belongs_to_database(value, database))
        });
    }
    is_database_scoped_catalog_family(family)
        && database_name_component(key)
            .is_some_and(|component| component.eq_ignore_ascii_case(database.as_bytes()))
}

pub(crate) fn validate_database_catalog_entry(
    key: &[u8],
    value: &[u8],
    database: &str,
) -> Result<(), CassieError> {
    let family = key_family(key).ok_or_else(|| {
        CassieError::Parse("database image contains an invalid catalog key".to_string())
    })?;
    if matches!(family, "collections" | "namespaces") {
        let values: Vec<String> = serde_json::from_slice(value).map_err(|error| {
            CassieError::Parse(format!("invalid database catalog list: {error}"))
        })?;
        if values
            .iter()
            .all(|value| catalog_name_belongs_to_database(value, database))
        {
            return Ok(());
        }
    } else if is_database_scoped_catalog_family(family)
        && database_name_component(key)
            .is_some_and(|component| component.eq_ignore_ascii_case(database.as_bytes()))
    {
        return Ok(());
    }
    Err(CassieError::Unsupported(format!(
        "database image catalog family '{family}' is not scoped to database '{database}'"
    )))
}

fn is_database_scoped_catalog_family(family: &str) -> bool {
    matches!(
        family,
        "schema"
            | "row-schema"
            | "projection"
            | "vector-index"
            | "index"
            | "view"
            | "sequence"
            | "constraints"
            | "namespace"
            | "cardinality"
            | "collection-meta"
            | "rollup"
            | "retention"
            | "collection-generation"
            | "maintenance-debt"
            | "graph"
    )
}

pub(super) fn catalog_name_belongs_to_database(name: &str, database: &str) -> bool {
    name.eq_ignore_ascii_case(database)
        || name
            .get(..database.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(database))
            && name.as_bytes().get(database.len()) == Some(&b'.')
}

pub(super) fn rewrite_key_component(key: &[u8], source: &str, target: &str) -> Vec<u8> {
    let mut rewritten = Vec::with_capacity(key.len() + target.len().saturating_sub(source.len()));
    for (index, component) in key_components(key).enumerate() {
        if index > 0 {
            rewritten.push(cntryl_lexkey::LexKey::SEPARATOR);
        }
        if index == DATABASE_NAME_COMPONENT_INDEX
            && component.eq_ignore_ascii_case(source.as_bytes())
        {
            rewritten.extend_from_slice(target.as_bytes());
        } else {
            rewritten.extend_from_slice(component);
        }
    }
    rewritten
}

pub(super) fn rewrite_catalog_value(
    family: &str,
    raw: &[u8],
    source: &str,
    target: &str,
) -> Result<Vec<u8>, CassieError> {
    let mut value: serde_json::Value = serde_json::from_slice(raw).map_err(|error| {
        CassieError::Parse(format!("invalid database catalog image value: {error}"))
    })?;
    match family {
        "projection" => rewrite_projection(&mut value, source, target),
        "vector-index" | "maintenance-debt" => {
            rewrite_fields(&mut value, &["collection"], source, target);
        }
        "index" => rewrite_fields(&mut value, &["collection", "name"], source, target),
        "view" | "sequence" | "namespace" | "collection-meta" => {
            rewrite_fields(&mut value, &["name"], source, target);
        }
        "constraints" => rewrite_constraints(&mut value, source, target),
        "rollup" => rewrite_fields(
            &mut value,
            &["name", "source_collection", "output_collection"],
            source,
            target,
        ),
        "retention" => rewrite_fields(&mut value, &["name", "collection"], source, target),
        "graph" => rewrite_fields(
            &mut value,
            &["name", "node_collection", "edge_collection"],
            source,
            target,
        ),
        "schema" | "row-schema" | "cardinality" | "collection-generation" => {}
        _ => {
            return Err(CassieError::Unsupported(format!(
                "database image catalog family '{family}' cannot be rewritten"
            )));
        }
    }
    serde_json::to_vec(&value).map_err(|error| CassieError::Parse(error.to_string()))
}

fn rewrite_projection(value: &mut serde_json::Value, source: &str, target: &str) {
    rewrite_fields(
        value,
        &["projection_id", "collection", "source_identity"],
        source,
        target,
    );
    let Some(object) = value.as_object_mut() else {
        return;
    };
    if let Some(generations) = object
        .get_mut("source_generations")
        .and_then(serde_json::Value::as_object_mut)
    {
        let previous = std::mem::take(generations);
        for (name, generation) in previous {
            generations.insert(rewrite_scoped_name(&name, source, target), generation);
        }
    }
    if let Some(materialized) = object.get_mut("materialized") {
        rewrite_fields(materialized, &["name", "output_collection"], source, target);
        if let Some(collections) = materialized
            .get_mut("source_collections")
            .and_then(serde_json::Value::as_array_mut)
        {
            rewrite_string_values(collections, source, target);
        }
    }
    if let Some(versions) = object
        .get_mut("versions")
        .and_then(serde_json::Value::as_array_mut)
    {
        for version in versions {
            rewrite_fields(version, &["output_collection"], source, target);
        }
    }
}

fn rewrite_constraints(value: &mut serde_json::Value, source: &str, target: &str) {
    let Some(constraints) = value.as_array_mut() else {
        return;
    };
    for constraint in constraints {
        rewrite_fields(
            constraint,
            &["default_sequence", "references_table"],
            source,
            target,
        );
    }
}

fn rewrite_fields(value: &mut serde_json::Value, fields: &[&str], source: &str, target: &str) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    for field in fields {
        if let Some(text) = object.get(*field).and_then(serde_json::Value::as_str) {
            let rewritten = rewrite_scoped_name(text, source, target);
            object.insert((*field).to_string(), serde_json::Value::String(rewritten));
        }
    }
}

fn rewrite_string_values(values: &mut [serde_json::Value], source: &str, target: &str) {
    for value in values {
        if let Some(text) = value.as_str() {
            *value = serde_json::Value::String(rewrite_scoped_name(text, source, target));
        }
    }
}

fn rewrite_scoped_name(value: &str, source: &str, target: &str) -> String {
    let has_source_prefix = value
        .get(..source.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(source))
        && value.as_bytes().get(source.len()) == Some(&b'.');
    if has_source_prefix {
        format!("{target}{}", &value[source.len()..])
    } else {
        value.to_string()
    }
}

pub(super) fn rewrite_string_list(
    raw: &[u8],
    source: &str,
    target: &str,
) -> Result<Vec<String>, CassieError> {
    let values: Vec<String> = serde_json::from_slice(raw)
        .map_err(|error| CassieError::Parse(format!("invalid database catalog list: {error}")))?;
    values
        .into_iter()
        .map(|value| {
            if !catalog_name_belongs_to_database(&value, source) {
                return Err(CassieError::Unsupported(format!(
                    "database image catalog name '{value}' is outside source database '{source}'"
                )));
            }
            Ok(rewrite_catalog_name(&value, source, target))
        })
        .collect()
}

fn rewrite_catalog_name(value: &str, source: &str, target: &str) -> String {
    value
        .get(source.len()..)
        .map_or_else(|| target.to_string(), |suffix| format!("{target}{suffix}"))
}

#[cfg(test)]
mod tests {
    use super::rewrite_catalog_value;

    fn rewrite(family: &str, value: &serde_json::Value) -> serde_json::Value {
        let raw = serde_json::to_vec(&value).expect("catalog value");
        let rewritten = rewrite_catalog_value(family, &raw, "analytics", "restored")
            .expect("rewritten catalog value");
        serde_json::from_slice(&rewritten).expect("rewritten json")
    }

    #[test]
    fn should_rewrite_only_database_scoped_index_fields() {
        // Arrange
        let value = serde_json::json!({
            "collection": "analytics.public.analytics",
            "name": "analytics",
            "field": "analytics",
            "fields": ["analytics"],
            "expressions": ["analytics + 1"],
            "include_fields": [],
            "predicate": "analytics = 'analytics'",
            "kind": "scalar",
            "unique": false,
            "options": {"note": "analytics"}
        });

        // Act
        let rewritten = rewrite("index", &value);

        // Assert
        assert_eq!(rewritten["collection"], "restored.public.analytics");
        assert_eq!(rewritten["name"], "analytics");
        assert_eq!(rewritten["field"], "analytics");
        assert_eq!(rewritten["expressions"][0], "analytics + 1");
        assert_eq!(rewritten["predicate"], "analytics = 'analytics'");
        assert_eq!(rewritten["options"]["note"], "analytics");
    }

    #[test]
    fn should_rewrite_constraint_references_without_changing_values_or_expressions() {
        // Arrange
        let value = serde_json::json!([{
            "field": "value",
            "default_value": "analytics",
            "default_expression": "'analytics'",
            "default_sequence": "analytics.public.analytics_sequence",
            "check": {"field": "value", "operator": "eq", "value": "analytics"},
            "references_table": "analytics.public.analytics",
            "references_field": "analytics"
        }]);

        // Act
        let rewritten = rewrite("constraints", &value);

        // Assert
        assert_eq!(rewritten[0]["default_value"], "analytics");
        assert_eq!(rewritten[0]["default_expression"], "'analytics'");
        assert_eq!(
            rewritten[0]["default_sequence"],
            "restored.public.analytics_sequence"
        );
        assert_eq!(rewritten[0]["check"]["value"], "analytics");
        assert_eq!(
            rewritten[0]["references_table"],
            "restored.public.analytics"
        );
        assert_eq!(rewritten[0]["references_field"], "analytics");
    }

    #[test]
    fn should_rewrite_projection_relations_without_changing_query_text() {
        // Arrange
        let value = serde_json::json!({
            "projection_id": "analytics.public.summary",
            "collection": "analytics.public.summary",
            "source_identity": "analytics.public.analytics",
            "source_generations": {"analytics.public.analytics": 4},
            "materialized": {
                "name": "analytics.public.summary",
                "query": "SELECT 'analytics' FROM analytics",
                "options": {"note": "analytics"},
                "output_collection": "analytics.public.__cassie_projection_summary_v1",
                "source_collections": ["analytics.public.analytics"]
            },
            "versions": [{
                "version_id": "v1",
                "output_collection": "analytics.public.__cassie_projection_summary_v1",
                "last_error": "analytics"
            }],
            "last_error": "analytics"
        });

        // Act
        let rewritten = rewrite("projection", &value);

        // Assert
        assert_eq!(rewritten["projection_id"], "restored.public.summary");
        assert_eq!(rewritten["collection"], "restored.public.summary");
        assert_eq!(
            rewritten["source_generations"]["restored.public.analytics"],
            4
        );
        assert_eq!(
            rewritten["materialized"]["query"],
            "SELECT 'analytics' FROM analytics"
        );
        assert_eq!(rewritten["materialized"]["options"]["note"], "analytics");
        assert_eq!(rewritten["versions"][0]["last_error"], "analytics");
        assert_eq!(rewritten["last_error"], "analytics");
    }
}
