//! Named domain types (#828).
//!
//! A [`DomainType`] is an immutable, versioned descriptor: a base type plus a
//! default, a NOT NULL flag and named CHECK constraints over `VALUE`. SQL
//! resolution flattens a domain to its base type for operators, functions and
//! wire output; only storage, casts and catalogs keep the domain identity.

mod descriptor;

pub use descriptor::{
    DomainCheck, DomainDefault, DomainRef, DomainType, DOMAIN_VALUE_COLUMN, MAX_DOMAIN_CHECKS,
};
