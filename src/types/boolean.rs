//! Canonical text input shared by supported Boolean adapters.
//!
//! Callers retain their own NULL, container and error policies.

/// Accepts the existing text Bind vocabulary without allocating a normalized string.
#[must_use]
pub(crate) fn parse_text(input: &str) -> Option<bool> {
    let value = input.trim();
    let is_prefix_of = |word: &str| {
        !value.is_empty()
            && word
                .get(..value.len())
                .is_some_and(|prefix| value.eq_ignore_ascii_case(prefix))
    };
    if value == "1"
        || value.eq_ignore_ascii_case("on")
        || is_prefix_of("true")
        || is_prefix_of("yes")
    {
        Some(true)
    } else if value == "0"
        || (value.len() >= 2 && is_prefix_of("off"))
        || is_prefix_of("false")
        || is_prefix_of("no")
    {
        Some(false)
    } else {
        None
    }
}
