use super::*;

impl Midge {
    pub fn put_operational_assignment(
        &self,
        metadata: OperationalAssignmentMeta,
    ) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        let value = serde_json::to_vec(&metadata)
            .map_err(|error| CassieError::Storage(format!("encode assignment: {error}")))?;
        tx.put(
            Self::operational_assignment_key(&metadata.assignment_id),
            value,
            None,
        )
        .map_err(CassieError::from)?;
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        Ok(())
    }

    pub fn list_operational_assignments(
        &self,
    ) -> Result<Vec<OperationalAssignmentMeta>, CassieError> {
        let entries = self.raw_scan_prefix(
            StorageFamily::Schema,
            &Self::operational_assignment_prefix(),
        )?;
        let mut out = Vec::with_capacity(entries.len());
        for (_key, raw_value) in entries {
            let Ok(record) = serde_json::from_slice(&raw_value) else {
                continue;
            };
            out.push(record);
        }
        out.sort_by(
            |left: &OperationalAssignmentMeta, right: &OperationalAssignmentMeta| {
                left.projection_id
                    .cmp(&right.projection_id)
                    .then_with(|| left.tenant.cmp(&right.tenant))
                    .then_with(|| left.partition_key.cmp(&right.partition_key))
                    .then_with(|| left.assignment_id.cmp(&right.assignment_id))
            },
        );
        Ok(out)
    }
}
