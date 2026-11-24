//! Engine catalog lookups using the shared admitted identifier matcher.
use cassie_core::catalog::controlled_name;
use cassie_core::request::text::TextRequest;

use crate::app::CassieError;
use crate::runtime::QueryExecutionControls;

pub(super) use controlled_name::Parts;

#[cfg(test)]
#[path = "controlled_name_tests.rs"]
mod tests;

fn request(controls: &QueryExecutionControls) -> TextRequest<'_> {
    TextRequest::new(controls.interruption(), controls.request_memory())
}

pub(crate) fn trim_checked<'a>(
    raw: &'a str,
    controls: &QueryExecutionControls,
) -> Result<&'a str, CassieError> {
    controlled_name::trim_checked(raw, &request(controls)).map_err(CassieError::from)
}

pub(super) fn parse_parts(
    raw: &str,
    controls: &QueryExecutionControls,
) -> Result<Option<Parts>, CassieError> {
    controlled_name::parse_parts(raw, &request(controls)).map_err(CassieError::from)
}

pub(crate) fn name_matches_controlled(
    stored: &str,
    requested: &str,
    controls: &QueryExecutionControls,
) -> Result<bool, CassieError> {
    controlled_name::name_matches_controlled(stored, requested, &request(controls))
        .map_err(CassieError::from)
}

impl super::Catalog {
    pub(crate) fn namespace_exists_controlled(
        &self,
        requested: &str,
        controls: &QueryExecutionControls,
    ) -> Result<bool, CassieError> {
        let namespaces = loop {
            controls.check_interruption()?;
            if let Some(namespaces) = self
                .namespaces
                .try_read_for(std::time::Duration::from_millis(2))
            {
                break namespaces;
            }
        };
        // Keep the common exact-name hash lookup while bounding its input scan.
        if requested.len() <= 4096 && namespaces.contains_key(requested) {
            return Ok(true);
        }
        for stored in namespaces.keys() {
            controls.check_interruption()?;
            if name_matches_controlled(stored, requested, controls)? {
                return Ok(true);
            }
        }
        Ok(false)
    }
}
