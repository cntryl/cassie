use cassie::types::Value;
use serde_json::json;

#[path = "support/constraint_ingress.rs"]
mod support;

use support::Fixture;

include!("constraint_ingress/contracts/semantics.rs");

include!("constraint_ingress/contracts/errors.rs");
