use super::metadata::Catalog;
pub use cassie_core::catalog::property_graph::*;

impl Catalog {
    pub fn register_property_graph(&self, metadata: PropertyGraphMeta) {
        self.property_graphs
            .write()
            .insert(metadata.name.to_ascii_lowercase(), metadata);
        self.bump_version();
    }

    pub fn unregister_property_graph(&self, name: &str) {
        self.property_graphs
            .write()
            .remove(&name.to_ascii_lowercase());
        self.bump_version();
    }

    /// The graph stored under exactly this canonical name.
    #[must_use]
    pub fn get_property_graph(&self, canonical: &str) -> Option<PropertyGraphMeta> {
        self.property_graphs
            .read()
            .get(&canonical.to_ascii_lowercase())
            .cloned()
    }

    #[must_use]
    pub fn list_property_graphs(&self) -> Vec<PropertyGraphMeta> {
        let mut out = self
            .property_graphs
            .read()
            .values()
            .cloned()
            .collect::<Vec<_>>();
        out.sort_by_key(|graph| graph.name.to_ascii_lowercase());
        out
    }

    /// Graphs that map `table`, matched by canonical name.
    #[must_use]
    pub fn property_graphs_mapping_table(&self, table: &str) -> Vec<PropertyGraphMeta> {
        self.list_property_graphs()
            .into_iter()
            .filter(|graph| {
                graph
                    .mapped_tables()
                    .iter()
                    .any(|mapped| mapped.eq_ignore_ascii_case(table))
            })
            .collect()
    }
}
