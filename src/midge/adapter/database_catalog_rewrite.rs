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
    if matches!(family, "function" | "procedure") {
        return program_catalog_name(value)
            .is_some_and(|name| catalog_name_belongs_to_database(&name, database));
    }
    if family == "maintenance-debt" {
        return serde_json::from_slice::<serde_json::Value>(value)
            .ok()
            .and_then(|record| record.get("collection")?.as_str().map(str::to_string))
            .is_some_and(|name| catalog_name_belongs_to_database(&name, database));
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
    } else if (matches!(family, "function" | "procedure")
        && program_catalog_name(value)
            .is_some_and(|name| catalog_name_belongs_to_database(&name, database)))
        || (family == "maintenance-debt"
            && serde_json::from_slice::<serde_json::Value>(value)
                .ok()
                .and_then(|record| record.get("collection")?.as_str().map(str::to_string))
                .is_some_and(|name| catalog_name_belongs_to_database(&name, database)))
        || (is_database_scoped_catalog_family(family)
            && database_name_component(key)
                .is_some_and(|component| component.eq_ignore_ascii_case(database.as_bytes())))
    {
        return Ok(());
    }
    Err(CassieError::Unsupported(format!(
        "database image catalog family '{family}' is not scoped to database '{database}'"
    )))
}

fn program_catalog_name(value: &[u8]) -> Option<String> {
    let value: serde_json::Value = serde_json::from_slice(value).ok()?;
    value.get("name")?.as_str().map(str::to_string)
}

fn is_database_scoped_catalog_family(family: &str) -> bool {
    matches!(
        family,
        "schema"
            | "row-schema"
            | "projection"
            | "projection-event"
            | "vector-index"
            | "index"
            | "index-publication"
            | "view"
            | "sequence"
            | "function"
            | "procedure"
            | "constraints"
            | "unique-constraint-publication"
            | "unique-constraint-cleanup"
            | "schema-cleanup"
            | "schema-operation"
            | "field-add-operation"
            | "field-rename-operation"
            | "field-drop-operation"
            | "namespace"
            | "cardinality"
            | "collection-meta"
            | "rollup"
            | "retention"
            | "maintenance-debt"
            | "graph"
    )
}

pub(crate) fn is_database_catalog_entry(key: &[u8]) -> bool {
    key_family(key).is_some_and(|family| {
        matches!(family, "collections" | "namespaces") || is_database_scoped_catalog_family(family)
    })
}

pub(super) fn catalog_name_belongs_to_database(name: &str, database: &str) -> bool {
    catalog_name_component_belongs_to_database(name.as_bytes(), database)
}

fn catalog_name_component_belongs_to_database(name: &[u8], database: &str) -> bool {
    name.eq_ignore_ascii_case(database.as_bytes())
        || name
            .get(..database.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(database.as_bytes()))
            && name.get(database.len()) == Some(&b'.')
}

pub(super) fn rewrite_key_component(key: &[u8], source: &str, target: &str) -> Vec<u8> {
    let mut rewritten = Vec::with_capacity(key.len() + target.len().saturating_sub(source.len()));
    for (index, component) in key_components(key).enumerate() {
        if index > 0 {
            rewritten.push(cntryl_lexkey::LexKey::SEPARATOR);
        }
        if index == DATABASE_NAME_COMPONENT_INDEX
            && catalog_name_component_belongs_to_database(component, source)
        {
            let suffix = if component.len() > source.len() {
                &component[source.len()..]
            } else {
                &[]
            };
            rewritten.extend_from_slice(target.as_bytes());
            rewritten.extend_from_slice(suffix);
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
    if matches!(family, "projection-event") {
        return Ok(raw.to_vec());
    }
    let mut value: serde_json::Value = serde_json::from_slice(raw).map_err(|error| {
        CassieError::Parse(format!("invalid database catalog image value: {error}"))
    })?;
    match family {
        "projection" => rewrite_projection(&mut value, source, target),
        "index-publication" => rewrite_index_publication(&mut value, source, target),
        "index" => rewrite_fields(&mut value, &["collection", "name"], source, target),
        "unique-constraint-publication" => {
            rewrite_fields(&mut value, &["collection"], source, target);
            if let Some(constraints) = value.get_mut("constraints") {
                rewrite_constraints(constraints, source, target);
            }
            if let Some(fields) = value.get_mut("fields") {
                rewrite_constraints(fields, source, target);
            }
        }
        "vector-index"
        | "maintenance-debt"
        | "unique-constraint-cleanup"
        | "field-add-operation"
        | "field-rename-operation"
        | "field-drop-operation" => {
            rewrite_fields(&mut value, &["collection"], source, target);
        }
        "schema-cleanup" => rewrite_schema_cleanup(&mut value, source, target),
        "schema-operation" => {
            rewrite_fields(&mut value, &["current_name", "next_name"], source, target);
        }
        "view" | "sequence" | "function" | "procedure" | "namespace" | "collection-meta" => {
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

fn rewrite_index_publication(value: &mut serde_json::Value, source: &str, target: &str) {
    if let Some(index) = value.get_mut("index") {
        rewrite_fields(index, &["collection", "name"], source, target);
    }
    if let Some(vector_backfill) = value.get_mut("vector_backfill") {
        if let Some(vector_index) = vector_backfill.get_mut("vector_index") {
            rewrite_fields(vector_index, &["collection"], source, target);
        }
    }
}

fn rewrite_schema_cleanup(value: &mut serde_json::Value, source: &str, target: &str) {
    let Some(actions) = value
        .get_mut("action")
        .and_then(serde_json::Value::as_object_mut)
    else {
        return;
    };
    for action in actions.values_mut() {
        rewrite_fields(action, &["table", "view"], source, target);
    }
}

fn rewrite_projection(value: &mut serde_json::Value, source: &str, target: &str) {
    rewrite_fields(value, &["projection_id", "collection"], source, target);
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
    if let Some(integrity) = object.get_mut("integrity") {
        rewrite_fields(integrity, &["target"], source, target);
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
    use super::{is_database_catalog_entry, rewrite_catalog_value, rewrite_key_component};

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
            "source_identity": "analytics.public.orders-stream",
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
            "integrity": {
                "target": "analytics.public.__cassie_projection_summary_v1",
                "last_error": "analytics"
            },
            "last_error": "analytics"
        });

        // Act
        let rewritten = rewrite("projection", &value);

        // Assert
        assert_eq!(rewritten["projection_id"], "restored.public.summary");
        assert_eq!(rewritten["collection"], "restored.public.summary");
        assert_eq!(
            rewritten["source_identity"],
            "analytics.public.orders-stream"
        );
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
        assert_eq!(
            rewritten["integrity"]["target"],
            "restored.public.__cassie_projection_summary_v1"
        );
        assert_eq!(rewritten["integrity"]["last_error"], "analytics");
        assert_eq!(rewritten["last_error"], "analytics");
    }

    #[test]
    fn should_rewrite_pending_index_publication_for_database_restore() {
        // Arrange
        let value = serde_json::json!({
            "index": {
                "collection": "analytics.public.items",
                "name": "items_by_value"
            },
            "vector_backfill": {
                "vector_index": {"collection": "analytics.public.items"}
            }
        });

        // Act
        let rewritten = rewrite("index-publication", &value);

        // Assert
        assert_eq!(rewritten["index"]["collection"], "restored.public.items");
        assert_eq!(rewritten["index"]["name"], "items_by_value");
        assert_eq!(
            rewritten["vector_backfill"]["vector_index"]["collection"],
            "restored.public.items"
        );
    }

    #[test]
    fn should_rewrite_pending_schema_cleanup_for_database_restore() {
        // Arrange
        let key = crate::midge::adapter::key_encoding::schema_cleanup_key(
            "analytics.public.items",
            "cleanup-1",
        );
        let value = serde_json::json!({
            "cleanup_id": "cleanup-1",
            "blocked_by_epoch": 18_446_744_073_709_551_615_u64,
            "action": {
                "DropTable": {
                    "table": "analytics.public.items",
                    "relation_id": 17
                }
            }
        });
        let raw = serde_json::to_vec(&value).expect("cleanup value");

        // Act
        let rewritten_key = rewrite_key_component(&key, "analytics", "restored");
        let rewritten = rewrite_catalog_value("schema-cleanup", &raw, "analytics", "restored")
            .expect("rewritten cleanup");
        let rewritten: serde_json::Value =
            serde_json::from_slice(&rewritten).expect("rewritten cleanup JSON");

        // Assert
        assert!(is_database_catalog_entry(&key));
        assert!(super::catalog_entry_belongs_to_database(
            &key,
            &raw,
            "analytics"
        ));
        assert_eq!(
            super::key_components(&rewritten_key)
                .nth(3)
                .expect("database component"),
            b"restored"
        );
        assert_eq!(
            rewritten["action"]["DropTable"]["table"],
            "restored.public.items"
        );
        assert_eq!(rewritten["action"]["DropTable"]["relation_id"], 17);
    }

    #[test]
    fn should_rewrite_unique_publication_for_database_restore() {
        // Arrange
        let publication_key =
            crate::midge::adapter::key_encoding::unique_constraint_publication_key(
                "analytics.public.items",
            );
        let publication = serde_json::json!({
            "collection": "analytics.public.items",
            "constraints": [{
                "default_sequence": "analytics.public.items_id_seq",
                "references_table": "analytics.public.accounts"
            }],
            "fields": []
        });
        let publication_raw = serde_json::to_vec(&publication).expect("publication value");

        // Act
        let rewritten_publication = rewrite("unique-constraint-publication", &publication);
        let rewritten_publication_key =
            rewrite_key_component(&publication_key, "analytics", "restored");

        // Assert
        assert!(is_database_catalog_entry(&publication_key));
        assert!(super::catalog_entry_belongs_to_database(
            &publication_key,
            &publication_raw,
            "analytics"
        ));
        assert_eq!(
            super::key_components(&rewritten_publication_key)
                .nth(3)
                .expect("database component"),
            b"restored"
        );
        assert_eq!(rewritten_publication["collection"], "restored.public.items");
        assert_eq!(
            rewritten_publication["constraints"][0]["references_table"],
            "restored.public.accounts"
        );
    }

    #[test]
    fn should_keep_unique_reservation_key_database_local_during_restore() {
        // Arrange
        let reservation = crate::midge::adapter::key_encoding::unique_constraint_reservation_key(
            "analytics.public.items",
            "email",
            &serde_json::json!("person@example.com"),
        )
        .expect("reservation key");

        // Act
        let rewritten_reservation = rewrite_key_component(&reservation, "analytics", "restored");

        // Assert
        assert!(!is_database_catalog_entry(&reservation));
        assert_eq!(rewritten_reservation, reservation);
        assert_eq!(
            super::key_components(&rewritten_reservation)
                .nth(3)
                .expect("database-local schema component"),
            b"public"
        );
    }

    #[test]
    fn should_rewrite_maintenance_debt_for_database_restore() {
        // Arrange
        let key = crate::midge::adapter::key_encoding::maintenance_debt_key(
            "analytics.public.items",
            "projection",
        );
        let value = serde_json::json!({
            "collection": "analytics.public.items",
            "artifact": "projection"
        });
        let raw = serde_json::to_vec(&value).expect("maintenance value");

        // Act
        let rewritten_key = rewrite_key_component(&key, "analytics", "restored");
        let rewritten = rewrite("maintenance-debt", &value);

        // Assert
        assert!(is_database_catalog_entry(&key));
        assert!(super::catalog_entry_belongs_to_database(
            &key,
            &raw,
            "analytics"
        ));
        assert_eq!(rewritten_key, key);
        assert_eq!(rewritten["collection"], "restored.public.items");
    }
}
