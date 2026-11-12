use super::super::{key_encoding, CassieError, Midge};

impl Midge {
    pub(crate) fn unique_scalar_index_reservation_owner(
        &self,
        collection: &str,
        index: &crate::catalog::IndexMeta,
        values: &[serde_json::Value],
    ) -> Result<Option<String>, CassieError> {
        let collection = self.canonical_collection_name(collection);
        let key = key_encoding::unique_index_reservation_key_for(
            &collection,
            index.reservation_identity()?,
            values,
        )?;
        let tx = self.begin_data_readonly_tx_for(&collection)?;
        let Some(owner) = tx.get(&key).map_err(CassieError::from)? else {
            return Ok(None);
        };
        String::from_utf8(owner.to_vec()).map(Some).map_err(|_| {
            CassieError::Parse(format!(
                "unique scalar index '{}' has a non-UTF-8 reservation owner",
                index.name
            ))
        })
    }
}
