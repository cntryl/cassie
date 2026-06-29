use super::{Cassie, CassieError};

impl Cassie {
    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn put_operational_assignment(
        &self,
        metadata: crate::catalog::OperationalAssignmentMeta,
    ) -> Result<(), CassieError> {
        validate_operational_assignment(&metadata)?;
        self.midge.put_operational_assignment(metadata.clone())?;
        self.catalog.register_operational_assignment(metadata);
        Ok(())
    }

    #[must_use]
    pub fn list_operational_assignments(&self) -> Vec<crate::catalog::OperationalAssignmentMeta> {
        self.catalog.list_operational_assignments()
    }
}

fn validate_operational_assignment(
    metadata: &crate::catalog::OperationalAssignmentMeta,
) -> Result<(), CassieError> {
    if metadata.assignment_id.trim().is_empty() {
        return Err(CassieError::Execution(
            "operational assignment requires assignment_id".to_string(),
        ));
    }
    if metadata.node_id.trim().is_empty() {
        return Err(CassieError::Execution(
            "operational assignment requires node_id".to_string(),
        ));
    }
    if metadata.projection_id.trim().is_empty() {
        return Err(CassieError::Execution(
            "operational assignment requires projection_id".to_string(),
        ));
    }
    Ok(())
}
