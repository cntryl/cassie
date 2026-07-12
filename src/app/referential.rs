use super::Cassie;

impl Cassie {
    pub(crate) fn referential_write_collections(&self, collection: &str) -> Vec<String> {
        let canonical_name = |name: &str| {
            self.catalog
                .get_schema(name)
                .map_or_else(|| name.to_string(), |schema| schema.collection)
        };
        let collection = canonical_name(collection);
        let mut collections = vec![collection.clone()];
        for constraint in self.catalog.get_constraints(&collection) {
            if let Some(referenced_table) = constraint.references_table {
                collections.push(canonical_name(&referenced_table));
            }
        }
        for candidate in self.catalog.list_collections_canonical() {
            if self
                .catalog
                .get_constraints(&candidate.name)
                .iter()
                .any(|constraint| {
                    constraint
                        .references_table
                        .as_deref()
                        .is_some_and(|referenced| {
                            canonical_name(referenced).eq_ignore_ascii_case(&collection)
                        })
                })
            {
                collections.push(candidate.name);
            }
        }
        collections
    }
}
