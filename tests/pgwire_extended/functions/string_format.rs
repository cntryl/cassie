//! Remaining PostgreSQL 18 `format()` syntax and error contracts for #803.

use super::{error_fields, run_script, wire, Step};

include!("string/format/semantics.rs");

include!("string/format/errors.rs");
