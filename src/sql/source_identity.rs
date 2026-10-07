//! Physical collection identity independent of the visible relation namespace.
use super::QuerySource;

/// Alias wrappers preserve an otherwise admitted collection source.
/// CTEs, derived queries, joins and functions retain their own capability boundary.
pub(crate) fn physical_collection(source: &QuerySource) -> Option<&str> {
    match source {
        QuerySource::Collection(name) => Some(name.as_str()),
        QuerySource::Aliased { source, .. } => physical_collection(source),
        _ => None,
    }
}
