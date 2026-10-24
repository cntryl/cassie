//! PostgreSQL 18 aggregate binding errors and functional dependencies over pgwire.

#[path = "core/resource_cancellation.rs"]
mod resource_cancellation;

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[path = "boolean/boolean_aggregate_cancellation.rs"]
mod boolean_aggregate_cancellation;
#[path = "boolean/boolean_or_type_error.rs"]
mod boolean_or_type_error;
#[path = "boolean/boolean_value_semantics.rs"]
mod boolean_value_semantics;
#[path = "modifiers/distinct_order_by_argument.rs"]
mod distinct_order_by_argument;

include!("core/aggregate_errors.rs");
include!("core/any_value.rs");

include!("boolean/aggregate_errors.rs");
