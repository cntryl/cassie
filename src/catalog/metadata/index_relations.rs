use super::Catalog;
use crate::catalog::{
    canonical_relation_name, local_name, name_matches, parse_name, IndexMeta, ParsedName,
    RelationId,
};

pub(super) fn rename_scoped_index_name(
    current_collection: &str,
    next_collection: &str,
    index_name: &str,
) -> String {
    let Some(current) = RelationId::parse_canonical(current_collection) else {
        return index_name.to_string();
    };
    let Some(next) = RelationId::parse_canonical(next_collection) else {
        return index_name.to_string();
    };
    let Some(index) = RelationId::parse_canonical(index_name) else {
        return index_name.to_string();
    };
    if index.database.eq_ignore_ascii_case(&current.database)
        && index.schema.eq_ignore_ascii_case(&current.schema)
    {
        canonical_relation_name(&next.database, &next.schema, &index.name)
    } else {
        index_name.to_string()
    }
}

impl Catalog {
    pub(crate) fn is_sql_materialized_view(&self, relation: &str) -> bool {
        self.get_materialized_projection(relation)
            .and_then(|projection| projection.materialized)
            .as_ref()
            .is_some_and(crate::catalog::MaterializedProjectionMeta::is_sql_materialized_view)
    }

    pub(crate) fn materialized_view_output_collection(&self, relation: &str) -> Option<String> {
        let projection = self.get_materialized_projection(relation)?;
        projection
            .materialized
            .as_ref()
            .is_some_and(crate::catalog::MaterializedProjectionMeta::is_sql_materialized_view)
            .then(|| projection.active_output_collection().map(str::to_string))
            .flatten()
    }

    pub(crate) fn logical_indexes_snapshot(&self) -> Vec<IndexMeta> {
        let mut out = Vec::new();
        for mut index in self.all_indexes_snapshot() {
            let Some(projection) = self.materialized_projection_for_output(&index.collection)
            else {
                out.push(index);
                continue;
            };
            let is_sql_view = projection
                .materialized
                .as_ref()
                .is_some_and(crate::catalog::MaterializedProjectionMeta::is_sql_materialized_view);
            if !is_sql_view {
                out.push(index);
                continue;
            }
            if projection.active_output_collection() == Some(index.collection.as_str()) {
                index.collection = projection.collection;
                out.push(index);
            }
        }
        sort_indexes(&mut out);
        out
    }

    pub fn register_index(&self, metadata: IndexMeta) {
        let mut indexes = self.indexes.write();
        indexes.insert(
            Self::index_key(&metadata.collection, &metadata.name),
            metadata,
        );
        self.bump_version();
    }

    pub fn unregister_index(&self, collection: &str, name: &str) {
        let collection = self
            .materialized_view_output_collection(collection)
            .unwrap_or_else(|| collection.to_string());
        let mut indexes = self.indexes.write();
        let key = Self::index_key(&collection, name);
        if indexes.remove(&key).is_none() {
            let matching_key = indexes
                .iter()
                .find(|(_, index)| {
                    name_matches(&index.collection, &collection) && name_matches(&index.name, name)
                })
                .map(|(stored_key, _)| stored_key.clone());
            if let Some(stored_key) = matching_key {
                indexes.remove(&stored_key);
            }
        }
        self.bump_version();
    }

    #[must_use]
    pub fn get_index(&self, collection: &str, name: &str) -> Option<IndexMeta> {
        let lookup_collection = self
            .materialized_view_output_collection(collection)
            .unwrap_or_else(|| collection.to_string());
        let indexes = self.indexes.read();
        indexes
            .get(&Self::index_key(&lookup_collection, name))
            .cloned()
            .or_else(|| {
                indexes
                    .values()
                    .find(|index| {
                        name_matches(&index.collection, &lookup_collection)
                            && name_matches(&index.name, name)
                    })
                    .cloned()
            })
            .map(|metadata| {
                if !self.is_sql_materialized_view(collection)
                    && matches!(parse_name(collection), Ok(ParsedName::Unqualified(_)))
                {
                    let mut display = metadata;
                    display.collection = local_name(&display.collection);
                    display
                } else {
                    metadata
                }
            })
    }

    #[must_use]
    pub fn list_indexes(&self, collection: &str) -> Vec<IndexMeta> {
        let lookup_collection = self
            .materialized_view_output_collection(collection)
            .unwrap_or_else(|| collection.to_string());
        let mut out = self
            .indexes
            .read()
            .values()
            .filter(|index| name_matches(&index.collection, &lookup_collection))
            .cloned()
            .collect::<Vec<_>>();
        out.sort_by_key(|index| index.name.to_ascii_lowercase());
        out
    }

    #[must_use]
    pub fn all_indexes_snapshot(&self) -> Vec<IndexMeta> {
        let mut out = self.indexes.read().values().cloned().collect::<Vec<_>>();
        sort_indexes(&mut out);
        out
    }
}

fn sort_indexes(indexes: &mut [IndexMeta]) {
    indexes.sort_by(|left, right| {
        left.collection
            .to_ascii_lowercase()
            .cmp(&right.collection.to_ascii_lowercase())
            .then_with(|| {
                left.name
                    .to_ascii_lowercase()
                    .cmp(&right.name.to_ascii_lowercase())
            })
    });
}
