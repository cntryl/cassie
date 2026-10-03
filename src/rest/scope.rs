use crate::app::{Cassie, CassieError, CassieSession};
use crate::catalog::{
    local_name, relation_database_name, relation_schema_name, split_identifier_path,
};

pub(crate) fn resolve_collection(
    cassie: &Cassie,
    session: &CassieSession,
    requested: &str,
) -> Result<String, CassieError> {
    let database = session
        .current_database()
        .unwrap_or(cassie.default_database.as_str());
    let parts = split_identifier_path(requested)
        .map_err(|error| CassieError::Parse(format!("invalid collection name: {error}")))?;
    if parts.is_empty() || parts.len() > 3 {
        return Err(CassieError::Parse(format!(
            "collection name must have one to three parts: {requested}"
        )));
    }

    let search_path = cassie.binding_search_path(session);
    let mut matches = cassie
        .catalog
        .list_collections_canonical()
        .into_iter()
        .filter(|collection| {
            relation_database_name(&collection.name)
                .is_some_and(|name| name.eq_ignore_ascii_case(database))
        })
        .filter(|collection| match parts.as_slice() {
            [name] => {
                local_name(&collection.name).eq_ignore_ascii_case(name)
                    && search_path.iter().any(|schema| {
                        relation_schema_name(&collection.name).eq_ignore_ascii_case(schema)
                    })
            }
            [schema, name] => {
                relation_schema_name(&collection.name).eq_ignore_ascii_case(schema)
                    && local_name(&collection.name).eq_ignore_ascii_case(name)
            }
            [requested_database, schema, name] => {
                requested_database.eq_ignore_ascii_case(database)
                    && relation_schema_name(&collection.name).eq_ignore_ascii_case(schema)
                    && local_name(&collection.name).eq_ignore_ascii_case(name)
            }
            _ => false,
        })
        .map(|collection| collection.name)
        .collect::<Vec<_>>();

    match matches.len() {
        0 => Err(CassieError::CollectionNotFound(requested.to_string())),
        1 => Ok(matches.pop().expect("one collection match")),
        _ => Err(CassieError::Unsupported(format!(
            "ambiguous REST collection '{requested}'"
        ))),
    }
}

pub(crate) fn list_collections(cassie: &Cassie, session: &CassieSession) -> Vec<String> {
    let database = session
        .current_database()
        .unwrap_or(cassie.default_database.as_str());
    let search_path = cassie.binding_search_path(session);
    cassie
        .catalog
        .list_collections_canonical()
        .into_iter()
        .filter(|collection| {
            relation_database_name(&collection.name)
                .is_some_and(|name| name.eq_ignore_ascii_case(database))
                && search_path.iter().any(|schema| {
                    relation_schema_name(&collection.name).eq_ignore_ascii_case(schema)
                })
        })
        .map(|collection| collection.name)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::resolve_collection;
    use crate::app::{Cassie, CassieError, CassieSession};
    use crate::catalog::{canonical_relation_name, DEFAULT_SCHEMA};
    use crate::types::{DataType, FieldSchema, Schema};

    fn cassie_with_collections(label: &str, names: &[&str]) -> Cassie {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "cassie-rest-scope-{label}-{}",
            uuid::Uuid::new_v4()
        ));
        let cassie = Cassie::new_with_data_dir(path).expect("cassie");
        let schema = Schema {
            fields: vec![FieldSchema {
                name: "id".to_string(),
                data_type: DataType::BigInt,
                nullable: false,
            }],
        };
        for name in names {
            if let Ok(crate::catalog::ParsedName::DatabaseQualified {
                database,
                schema: namespace,
                ..
            }) = crate::catalog::parse_name(name)
            {
                cassie.catalog.register_namespace(
                    &crate::catalog::canonical_schema_name(&database, &namespace),
                    None,
                );
            }
            cassie.register_collection(*name, schema.clone());
        }
        cassie
    }

    #[test]
    fn should_resolve_rest_collection_from_search_path() {
        // Arrange
        let collection = canonical_relation_name("cassie", "private", "events");
        let cassie = cassie_with_collections("search-path", &[&collection]);
        let session =
            CassieSession::authenticated("reader".to_string(), Some("cassie".to_string()), false);
        session.set_search_path(vec!["private".to_string(), DEFAULT_SCHEMA.to_string()]);

        // Act
        let resolved = resolve_collection(&cassie, &session, "events");

        // Assert
        assert_eq!(resolved.expect("resolve collection"), collection);
    }

    #[test]
    fn should_reject_ambiguous_rest_collection_scope() {
        // Arrange
        let first = canonical_relation_name("cassie", "first", "events");
        let second = canonical_relation_name("cassie", "second", "events");
        let cassie = cassie_with_collections("ambiguous", &[&first, &second]);
        let session =
            CassieSession::authenticated("reader".to_string(), Some("cassie".to_string()), false);
        session.set_search_path(vec!["first".to_string(), "second".to_string()]);

        // Act
        let error = resolve_collection(&cassie, &session, "events").expect_err("ambiguous scope");

        // Assert
        assert!(
            matches!(error, CassieError::Unsupported(message) if message.contains("ambiguous"))
        );
    }

    #[test]
    fn should_reject_cross_database_rest_collection_scope() {
        // Arrange
        let collection = canonical_relation_name("other", DEFAULT_SCHEMA, "events");
        let cassie = cassie_with_collections("cross-database", &[&collection]);
        let session =
            CassieSession::authenticated("reader".to_string(), Some("cassie".to_string()), false);

        // Act
        let error =
            resolve_collection(&cassie, &session, &collection).expect_err("foreign database");

        // Assert
        assert!(matches!(error, CassieError::CollectionNotFound(name) if name == collection));
    }

    #[test]
    fn should_keep_quoted_dotted_rest_collection_distinct_from_schema_qualified_name() {
        // Arrange
        let dotted = canonical_relation_name("cassie", DEFAULT_SCHEMA, "triage.dot");
        let qualified = canonical_relation_name("cassie", "triage", "dot");
        let cassie = cassie_with_collections("quoted-dot", &[&dotted, &qualified]);
        let session =
            CassieSession::authenticated("reader".to_string(), Some("cassie".to_string()), false);
        session.set_search_path(vec![DEFAULT_SCHEMA.to_string(), "triage".to_string()]);

        // Act
        let quoted = resolve_collection(&cassie, &session, "\"triage.dot\"");
        let schema_qualified = resolve_collection(&cassie, &session, "triage.dot");

        // Assert
        assert_eq!(quoted.expect("resolve quoted collection"), dotted);
        assert_eq!(
            schema_qualified.expect("resolve schema-qualified collection"),
            qualified
        );
    }
}
