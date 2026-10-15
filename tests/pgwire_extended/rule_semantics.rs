//! PostgreSQL 18 rewrite-rule ordering, recursion, catalog, and rollback contracts.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[path = "protocol/alter_rule_lifecycle/semantics.rs"]
mod alter_rule_lifecycle;

fn command_tags(frames: &[(u8, Vec<u8>)]) -> Vec<String> {
    frames
        .iter()
        .filter(|frame| frame.0 == b'C')
        .map(|(_, payload)| {
            let terminator = payload
                .iter()
                .position(|byte| *byte == 0)
                .expect("CommandComplete tag terminator");
            String::from_utf8(payload[..terminator].to_vec()).expect("command tag is UTF-8")
        })
        .collect()
}

include!("protocol/rule_semantics/semantics.rs");

include!("ordering/rule_semantics/semantics.rs");

include!("ctes/rule_semantics/semantics.rs");

include!("protocol/rule_semantics/transactions.rs");
