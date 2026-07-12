use super::{
    collect_scan, decode_projected_row, decode_projected_row_with_aliases, decode_row,
    key_encoding, CassieError, DocumentRef, HashSet, Midge, Query, RowDecode,
};

impl Midge {
    pub(crate) fn scan_rows_until<E, F>(
        &self,
        collection: &str,
        decode: RowDecode,
        mut visit: F,
    ) -> Result<usize, E>
    where
        E: From<CassieError>,
        F: FnMut(DocumentRef) -> Result<bool, E>,
    {
        let collection = self.canonical_collection_name(collection);
        if self
            .collection_uses_column_store(&collection)
            .map_err(E::from)?
        {
            let (batches, _) = self
                .scan_rows_batched(&collection, 1024, decode, None, None)
                .map_err(E::from)?;
            let mut emitted = 0usize;
            for document in batches.into_iter().flatten() {
                emitted += 1;
                if !visit(document)? {
                    break;
                }
            }
            return Ok(emitted);
        }

        let row_schema = self.row_schema(&collection).map_err(E::from)?;
        let (projection, include_historical_aliases) = decode.into_projection();
        let tx = self
            .begin_data_readonly_tx_for(&collection)
            .map_err(E::from)?;
        let mut seen_ids = HashSet::new();
        let mut emitted = 0usize;

        for (prefix, include_seen) in [
            (Self::row_prefix(&collection), true),
            (Self::doc_prefix(&collection), false),
        ] {
            let scan = tx
                .scan(&Query::new().prefix(prefix.clone().into()))
                .map_err(CassieError::from)
                .map_err(E::from)?;
            let iter = collect_scan(scan).map_err(E::from)?;
            for (raw_key, raw_value) in iter {
                let Some(id) = key_encoding::utf8_suffix_after_prefix(&raw_key, &prefix) else {
                    continue;
                };
                if id.is_empty() || (!include_seen && seen_ids.contains(&id)) {
                    continue;
                }
                seen_ids.insert(id.clone());

                let payload = match projection.as_ref() {
                    Some(projection) if include_historical_aliases => {
                        decode_projected_row_with_aliases(&row_schema, &raw_value, projection)
                            .map_err(E::from)?
                    }
                    Some(projection) => decode_projected_row(&row_schema, &raw_value, projection)
                        .map_err(E::from)?,
                    None => decode_row(&row_schema, &raw_value).map_err(E::from)?,
                };
                emitted += 1;
                if !visit(DocumentRef { id, payload })? {
                    return Ok(emitted);
                }
            }
        }

        Ok(emitted)
    }
}
