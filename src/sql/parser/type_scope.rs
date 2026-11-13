//! Scoped resolution of user-defined composite type names while parsing.
//!
//! The SQL parser is context free. Entry points that parse user SQL for a
//! session install a resolver here so a type name such as `address` or
//! `app.address` parses to the immutable composite descriptor visible to that
//! session at parse time.

use std::cell::RefCell;
use std::rc::Rc;

use crate::types::CompositeRef;

/// Maps raw type text to the composite type visible in the current scope.
pub type TypeResolver = Rc<dyn Fn(&str) -> Option<CompositeRef>>;

thread_local! {
    static RESOLVER: RefCell<Option<TypeResolver>> = const { RefCell::new(None) };
}

struct Restore(Option<TypeResolver>);

impl Drop for Restore {
    fn drop(&mut self) {
        RESOLVER.with(|slot| *slot.borrow_mut() = self.0.take());
    }
}

/// Runs `parse` with `resolver` installed, restoring the previous scope after.
pub fn with_type_scope<R>(resolver: TypeResolver, parse: impl FnOnce() -> R) -> R {
    let previous = RESOLVER.with(|slot| slot.borrow_mut().replace(resolver));
    let _restore = Restore(previous);
    parse()
}

/// Parses a declared data type name, resolving composite and domain names
/// against the installed scopes.
///
/// # Errors
///
/// Returns the parse error when `raw` names neither a built-in nor a scoped type.
pub fn parse_data_type_in_scope(raw: &str) -> Result<crate::types::DataType, super::SqlError> {
    super::domain_scope::parse_declared_data_type(raw)
}

/// Resolves `raw` against the installed scope, if any.
pub(super) fn resolve_named_type(raw: &str) -> Option<CompositeRef> {
    let resolver = RESOLVER.with(|slot| slot.borrow().clone())?;
    resolver(raw.trim())
}
