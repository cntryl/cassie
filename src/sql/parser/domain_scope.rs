//! Scoped resolution of user-defined domain names while parsing (#828).
//!
//! Like the composite type scope, an entry point that parses user SQL for a
//! session installs a resolver so a type name such as `positive` or
//! `app.positive` in a column definition or a cast parses to the immutable
//! domain descriptor visible to that session. Unscoped parses fail closed with
//! an unknown-type error.

use std::cell::RefCell;
use std::rc::Rc;

use super::schema::parse_data_type;
use super::SqlError;
use crate::types::{DataType, DomainRef};

/// Maps raw type text to the domain visible in the current scope.
pub type DomainResolver = Rc<dyn Fn(&str) -> Option<DomainRef>>;

thread_local! {
    static RESOLVER: RefCell<Option<DomainResolver>> = const { RefCell::new(None) };
}

struct Restore(Option<DomainResolver>);

impl Drop for Restore {
    fn drop(&mut self) {
        RESOLVER.with(|slot| *slot.borrow_mut() = self.0.take());
    }
}

/// Runs `parse` with `resolver` installed, restoring the previous scope after.
pub fn with_domain_scope<R>(resolver: DomainResolver, parse: impl FnOnce() -> R) -> R {
    let previous = RESOLVER.with(|slot| slot.borrow_mut().replace(resolver));
    let _restore = Restore(previous);
    parse()
}

fn resolve_named_domain(raw: &str) -> Option<DomainRef> {
    let resolver = RESOLVER.with(|slot| slot.borrow().clone())?;
    resolver(raw.trim())
}

/// Parses a column or cast target type, resolving a domain name when the text
/// is not a built-in type.
pub(super) fn parse_declared_data_type(raw: &str) -> Result<DataType, SqlError> {
    let error = match parse_data_type(raw) {
        Ok(data_type) => return Ok(data_type),
        Err(error) => error,
    };
    let trimmed = raw.trim();
    if let Some(element) = trimmed.strip_suffix("[]") {
        if resolve_named_domain(element).is_some() {
            return Err(SqlError::unsupported(
                "arrays of domain types are not supported".into(),
            ));
        }
        return Err(error);
    }
    resolve_named_domain(trimmed).map_or(Err(error), |domain| Ok(DataType::Domain(domain)))
}
