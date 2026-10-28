//! PostgreSQL 18 string-function parameter and result metadata tracked by #803.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[path = "string/format_parameter_wires.rs"]
mod format_parameter_wires;
#[path = "string/strpos_result_identity.rs"]
mod strpos_result_identity;
#[path = "string/text_selection.rs"]
mod text_selection;

include!("string/parameters.rs");

include!("string/errors.rs");

include!("string/nulls.rs");

include!("string/semantics.rs");

include!("string/resources.rs");

include!("string/cancellation.rs");
