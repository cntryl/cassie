//! Focused PostgreSQL 18 pgwire inventory gaps for #762, #796, #798, and #799.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

include!("subqueries/query/nulls.rs");

include!("subqueries/query/semantics.rs");

include!("types/arrays/query/metadata.rs");

include!("grammar/query/semantics.rs");

include!("patterns/query/parameters.rs");

include!("functions/math/query/metadata.rs");

include!("protocol/query/errors.rs");

include!("grammar/query/metadata.rs");
