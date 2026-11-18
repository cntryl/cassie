//! CTE declarations and bound Cte sources already carry resolved identifier data.

use super::IdentifierPath;

/// Only an unbound, unqualified collection can name a visible CTE. Do not parse
/// a declaration or bound Cte String here: embedded dots and quotes are data.
pub(crate) fn reference_name(path: &IdentifierPath) -> Option<&str> {
    match path.components() {
        [component] => Some(component.value()),
        _ => None,
    }
}

/// Encodes resolved data for existing SQL-text qualification interfaces. This
/// rendering is never a namespace key or producer identity.
pub(crate) fn qualifier(name: &str) -> String {
    crate::catalog::canonical_identifier_component(name)
}
