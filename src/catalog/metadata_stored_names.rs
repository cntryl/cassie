use std::collections::HashMap;

use super::Catalog;
use crate::catalog::{name_matches, ProjectionKind};

impl Catalog {
    /// Returns the stored collection key for `collection`, preferring an exact
    /// key and otherwise using the same matching as `exists`.
    #[must_use]
    pub fn stored_collection_name(&self, collection: &str) -> Option<String> {
        stored_key(&self.collections.read(), collection, |_| true)
    }

    /// Returns the stored table, view, or materialized projection key for
    /// `name`, using the same matching as `relation_exists`.
    #[must_use]
    pub fn stored_relation_name(&self, name: &str) -> Option<String> {
        self.stored_collection_name(name)
            .or_else(|| stored_key(&self.views.read(), name, |_| true))
            .or_else(|| {
                stored_key(&self.projections.read(), name, |projection| {
                    projection.kind == ProjectionKind::Materialized
                })
            })
    }

    /// Returns every stored table, view, or materialized projection key that
    /// matches `name`, sorted. More than one entry means the reference is
    /// ambiguous and cannot be resolved.
    #[must_use]
    pub fn matching_relation_names(&self, name: &str) -> Vec<String> {
        let collections = matching_keys(&self.collections.read(), name, |_| true);
        if collections.len() == 1 && collections[0] == name {
            return collections;
        }
        let mut matches = collections;
        matches.extend(matching_keys(&self.views.read(), name, |_| true));
        matches.extend(matching_keys(
            &self.projections.read(),
            name,
            |projection| projection.kind == ProjectionKind::Materialized,
        ));
        matches.sort();
        matches.dedup();
        matches
    }

    /// Returns the stored namespace key for `namespace`, using the same
    /// matching as `namespace_exists`.
    #[must_use]
    pub fn stored_namespace_name(&self, namespace: &str) -> Option<String> {
        stored_key(&self.namespaces.read(), namespace, |_| true)
    }
}

/// Returns the single stored key matching `requested`, or `None` when nothing
/// matches or when more than one stored key matches. Resolution never picks an
/// arbitrary match: an ambiguous reference resolves to nothing.
fn stored_key<V>(
    entries: &HashMap<String, V>,
    requested: &str,
    eligible: impl Fn(&V) -> bool,
) -> Option<String> {
    let mut matches = matching_keys(entries, requested, eligible);
    (matches.len() == 1).then(|| matches.remove(0))
}

/// Returns every stored key matching `requested`, sorted, or just the exact key
/// when one exists.
fn matching_keys<V>(
    entries: &HashMap<String, V>,
    requested: &str,
    eligible: impl Fn(&V) -> bool,
) -> Vec<String> {
    if entries.get(requested).is_some_and(&eligible) {
        return vec![requested.to_string()];
    }
    let mut matches = entries
        .iter()
        .filter(|(stored, value)| eligible(value) && name_matches(stored, requested))
        .map(|(stored, _)| stored.clone())
        .collect::<Vec<_>>();
    matches.sort();
    matches
}

#[cfg(test)]
mod tests {
    use crate::app::CassieError;
    use crate::catalog::Catalog;
    use crate::types::DataType;

    fn catalog_with_collection(name: &str) -> Catalog {
        let catalog = Catalog::new();
        catalog.register_collection(name, vec![("title".to_string(), DataType::Text)]);
        catalog
    }

    #[test]
    fn should_return_stored_collection_name_for_differently_cased_reference() {
        // Arrange
        let catalog = catalog_with_collection("postgres.public.Users");

        // Act
        let stored = catalog.stored_relation_name("postgres.public.users");

        // Assert
        assert_eq!(stored.as_deref(), Some("postgres.public.Users"));
    }

    #[test]
    fn should_return_stored_collection_name_for_unqualified_reference() {
        // Arrange
        let catalog = catalog_with_collection("postgres.public.Users");

        // Act
        let stored = catalog.stored_collection_name("users");

        // Assert
        assert_eq!(stored.as_deref(), Some("postgres.public.Users"));
    }

    #[test]
    fn should_reject_unregistering_collection_without_exact_stored_key() {
        // Arrange
        let catalog = catalog_with_collection("postgres.public.Users");

        // Act
        let result = catalog.unregister_collection("postgres.public.users");

        // Assert
        assert!(matches!(
            result,
            Err(CassieError::CollectionNotFound(name)) if name == "postgres.public.users"
        ));
    }

    #[test]
    fn should_reject_renaming_collection_without_exact_stored_key() {
        // Arrange
        let catalog = catalog_with_collection("postgres.public.Users");

        // Act
        let result = catalog.rename_collection("postgres.public.users", "postgres.public.people");

        // Assert
        assert!(matches!(
            result,
            Err(CassieError::CollectionNotFound(name)) if name == "postgres.public.users"
        ));
        assert!(catalog.exists("postgres.public.Users"));
        assert!(!catalog.exists("postgres.public.people"));
    }
}
