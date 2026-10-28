//! Existing CTE bindings and borrowed controls shared by nested binding scopes.
use super::CteBinding;
use crate::runtime::QueryExecutionControls;
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};

impl CteBinding {
    pub(super) fn has_implicit_identity(&self) -> bool {
        use crate::types::row_identity::{is_legacy_id_column, is_row_identity_column};
        !self.visible.iter().any(|name| is_legacy_id_column(name))
            && self
                .row_fields
                .iter()
                .any(|field| is_row_identity_column(&field.name))
    }
}

#[derive(Clone, Default)]
pub(super) struct CteScope<'a> {
    bindings: HashMap<String, CteBinding>,
    controls: Option<&'a QueryExecutionControls>,
}
impl<'a> CteScope<'a> {
    pub(super) fn new(controls: Option<&'a QueryExecutionControls>) -> Self {
        Self {
            bindings: HashMap::new(),
            controls,
        }
    }
    pub(super) fn controls(&self) -> Option<&'a QueryExecutionControls> {
        self.controls
    }
}
impl Deref for CteScope<'_> {
    type Target = HashMap<String, CteBinding>;
    fn deref(&self) -> &Self::Target {
        &self.bindings
    }
}
impl DerefMut for CteScope<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.bindings
    }
}
impl FromIterator<(String, CteBinding)> for CteScope<'_> {
    fn from_iter<T: IntoIterator<Item = (String, CteBinding)>>(iter: T) -> Self {
        Self {
            bindings: iter.into_iter().collect(),
            controls: None,
        }
    }
}
