use crate::executor::semantic::SemanticValue;
use crate::types::Value;

/// Retained accumulator bytes before and after an update or merge.
#[derive(Clone, Copy, Default)]
pub(in crate::executor::execution) struct RetainedChange {
    pub(super) before: usize,
    pub(super) after: usize,
}

impl RetainedChange {
    pub(super) fn replaced(
        before: Option<&Value>,
        after: &Value,
        old_key: Option<&SemanticValue>,
        new_key: Option<&SemanticValue>,
    ) -> Self {
        Self {
            before: before
                .map_or(0, value_retained_bytes)
                .saturating_add(old_key.map_or(0, SemanticValue::estimated_bytes)),
            after: value_retained_bytes(after)
                .saturating_add(new_key.map_or(0, SemanticValue::estimated_bytes)),
        }
    }

    pub(super) fn absorb(&mut self, other: Self) {
        self.before = self.before.saturating_add(other.before);
        self.after = self.after.saturating_add(other.after);
    }
}

pub(super) fn value_retained_bytes(value: &Value) -> usize {
    super::group_memory::json_bytes(value)
}
