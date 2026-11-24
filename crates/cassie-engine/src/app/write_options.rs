use crate::midge::adapter::DocumentWriteBatchOptions;
use std::collections::BTreeSet;

use super::Cassie;

impl Cassie {
    pub(crate) fn document_write_options(&self, collection: &str) -> DocumentWriteBatchOptions {
        let mut options = DocumentWriteBatchOptions::sync(self.midge.write_options_sync());
        self.add_derived_maintenance_debt_options(collection, &mut options);
        options
    }

    pub(crate) fn buffered_document_write_options(
        &self,
        collection: &str,
    ) -> DocumentWriteBatchOptions {
        let mut options = DocumentWriteBatchOptions::buffered(self.midge.write_options_buffered());
        self.add_derived_maintenance_debt_options(collection, &mut options);
        options
    }

    pub(crate) fn document_write_options_for_collections(
        &self,
        collections: &[String],
    ) -> DocumentWriteBatchOptions {
        let mut options = DocumentWriteBatchOptions::sync(self.midge.write_options_sync());
        for collection in collections {
            self.add_derived_maintenance_debt_options(collection, &mut options);
        }
        options
    }

    fn add_derived_maintenance_debt_options(
        &self,
        collection: &str,
        options: &mut DocumentWriteBatchOptions,
    ) {
        let collection = self
            .catalog
            .get_schema(collection)
            .map_or_else(|| collection.to_string(), |schema| schema.collection);
        let rollups = self.catalog.list_rollups_for_source(&collection);
        if !rollups.is_empty() {
            options.record_rollup_maintenance_debt = true;
            let mut fields = BTreeSet::new();
            for rollup in rollups {
                fields.insert(rollup.timestamp_field);
                fields.extend(rollup.group_keys);
            }
            options.rollup_change_fields = fields.into_iter().collect();
        }
        if self
            .catalog
            .list_projection_metadata()
            .into_iter()
            .filter_map(|projection| projection.materialized)
            .any(|materialized| {
                materialized
                    .source_collections
                    .iter()
                    .any(|source| source.eq_ignore_ascii_case(&collection))
            })
        {
            options.record_materialized_projection_maintenance_debt = true;
        }
    }
}
