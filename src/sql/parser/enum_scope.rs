//! Scoped resolution of user-defined enum names while parsing (#827).
//!
//! Like the composite and domain scopes, an entry point that parses user SQL
//! for a session installs a resolver so a type name such as `mood`,
//! `app.mood` or `mood[]` in a column definition, cast or parameter type
//! parses to the immutable enum descriptor visible to that session.
//! Unscoped parses fail closed with an unknown-type error.

use std::cell::RefCell;
use std::rc::Rc;

use super::SqlError;
use crate::types::{DataType, EnumRef};

/// Maps raw type text to the enum visible in the current scope.
pub type EnumResolver = Rc<dyn Fn(&str) -> Option<EnumRef>>;

thread_local! {
    static RESOLVER: RefCell<Option<EnumResolver>> = const { RefCell::new(None) };
}

struct Restore(Option<EnumResolver>);

impl Drop for Restore {
    fn drop(&mut self) {
        RESOLVER.with(|slot| *slot.borrow_mut() = self.0.take());
    }
}

/// Runs `parse` with `resolver` installed, restoring the previous scope after.
pub fn with_enum_scope<R>(resolver: EnumResolver, parse: impl FnOnce() -> R) -> R {
    let previous = RESOLVER.with(|slot| slot.borrow_mut().replace(resolver));
    let _restore = Restore(previous);
    parse()
}

fn resolve_named_enum(raw: &str) -> Option<EnumRef> {
    let resolver = RESOLVER.with(|slot| slot.borrow().clone())?;
    resolver(raw.trim())
}

/// The element type text of an array spelling (`name[]` or `name ARRAY`).
fn array_element(trimmed: &str) -> Option<&str> {
    if let Some(element) = trimmed.strip_suffix("[]") {
        // PostgreSQL does not enforce declared dimensions: `t[][]` is `t[]`.
        let mut element = element.trim_end();
        while let Some(inner) = element.strip_suffix("[]") {
            element = inner.trim_end();
        }
        return Some(element);
    }
    let split = trimmed.len().checked_sub(" array".len())?;
    let (element, suffix) = trimmed.split_at_checked(split)?;
    suffix
        .eq_ignore_ascii_case(" array")
        .then_some(element.trim_end())
}

/// Resolves `raw` as an enum type or an array of one.
pub(super) fn parse_enum_type(raw: &str) -> Option<DataType> {
    let trimmed = raw.trim();
    if let Some(element) = array_element(trimmed) {
        return resolve_named_enum(element)
            .map(|descriptor| DataType::Array(Box::new(DataType::Enum(descriptor))));
    }
    resolve_named_enum(trimmed).map(DataType::Enum)
}

/// The error for a composite type attribute: enums cannot be attributes in
/// this version (`0A000`); any other failure is passed through.
pub(super) fn enum_field_error(type_text: &str, error: SqlError) -> SqlError {
    if parse_enum_type(type_text).is_some() {
        return SqlError::unsupported(
            "enum types as composite type attributes are not supported".into(),
        );
    }
    error
}

/// The error for `ALTER COLUMN ... TYPE` naming an enum: converting stored
/// values to enum member ids is not part of this version (`0A000`); any other
/// failure is passed through.
pub(super) fn enum_column_type_error(type_text: &str, error: SqlError) -> SqlError {
    if parse_enum_type(type_text).is_some() {
        return SqlError::unsupported("ALTER COLUMN TYPE to an enum type is not supported".into());
    }
    error
}

#[cfg(test)]
#[path = "enum_scope_tests.rs"]
mod tests;
