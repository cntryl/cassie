use std::collections::HashMap;

use super::Catalog;

impl Catalog {
    /// Returns the stored collection key that matches `collection` ignoring case.
    #[must_use]
    pub fn stored_collection_name(&self, collection: &str) -> Option<String> {
        stored_key(&self.collections.read(), collection)
    }

    /// Returns the stored table or view key that matches `name` ignoring case.
    #[must_use]
    pub fn stored_relation_name(&self, name: &str) -> Option<String> {
        self.stored_collection_name(name)
            .or_else(|| stored_key(&self.views.read(), name))
    }

    /// Returns the stored namespace key that matches `namespace` ignoring case.
    #[must_use]
    pub fn stored_namespace_name(&self, namespace: &str) -> Option<String> {
        stored_key(&self.namespaces.read(), namespace)
    }
}

fn stored_key<V>(entries: &HashMap<String, V>, requested: &str) -> Option<String> {
    if entries.contains_key(requested) {
        return Some(requested.to_string());
    }
    entries
        .keys()
        .find(|stored| stored.eq_ignore_ascii_case(requested))
        .cloned()
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
