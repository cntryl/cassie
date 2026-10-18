//! PostgreSQL 18 row-subquery and row-comparison pgwire semantics for #762/#800.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

include!("../ordering/row_subquery_wire_semantics/nulls.rs");

include!("row_subquery_wire_semantics/nulls.rs");
