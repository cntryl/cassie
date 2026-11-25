use crate::catalog::Catalog;
pub use cassie_core::catalog::operational::{
    OperationalAssignmentMeta, OperationalAssignmentState,
};

impl Catalog {
    pub fn register_operational_assignment(&self, metadata: OperationalAssignmentMeta) {
        self.operational_assignments
            .write()
            .insert(metadata.assignment_id.clone(), metadata);
        self.bump_version();
    }

    #[must_use]
    pub fn list_operational_assignments(&self) -> Vec<OperationalAssignmentMeta> {
        let mut out = self
            .operational_assignments
            .read()
            .values()
            .cloned()
            .collect::<Vec<_>>();
        out.sort_by(|left, right| {
            left.projection_id
                .cmp(&right.projection_id)
                .then_with(|| left.tenant.cmp(&right.tenant))
                .then_with(|| left.partition_key.cmp(&right.partition_key))
                .then_with(|| left.assignment_id.cmp(&right.assignment_id))
        });
        out
    }
}
