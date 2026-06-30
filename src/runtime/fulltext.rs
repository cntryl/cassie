use super::{AnalyzerConfig, Hash, HashMap};

#[derive(Debug, Clone, Default)]
pub struct FulltextIndexOptions {
    pub field_boost: HashMap<String, f64>,
    pub field_k1: HashMap<String, f64>,
    pub field_b: HashMap<String, f64>,
    pub field_analyzer: HashMap<String, AnalyzerConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FulltextIndexOptionsCacheKey {
    pub schema_epoch: u64,
    pub collection: String,
    pub fields: Vec<String>,
}

impl FulltextIndexOptionsCacheKey {
    pub fn new<I>(schema_epoch: u64, collection: &str, fields: I) -> Self
    where
        I: IntoIterator<Item = String>,
    {
        let mut normalized_fields = fields
            .into_iter()
            .map(|field| field.to_ascii_lowercase())
            .collect::<Vec<_>>();
        normalized_fields.sort();
        normalized_fields.dedup();

        Self {
            schema_epoch,
            collection: collection.to_ascii_lowercase(),
            fields: normalized_fields,
        }
    }
}
