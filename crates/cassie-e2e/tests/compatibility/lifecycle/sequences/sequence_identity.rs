use super::*;

#[path = "remaining.rs"]
mod remaining_contracts;
#[path = "session_local_value_history/semantics.rs"]
mod session_local_value_history;

include!("sequence_identity/nulls.rs");

include!("sequence_identity/resources.rs");

include!("sequence_identity/restart.rs");

include!("sequence_identity/semantics.rs");
