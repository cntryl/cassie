use super::*;

#[path = "catalog_identity_lookups.rs"]
mod catalog_identity_lookups;
#[path = "../comments/comment_catalog_coverage.rs"]
mod comment_catalog_coverage;
#[path = "../comments/comment_owner_lifecycle.rs"]
mod comment_owner_lifecycle;
#[path = "../comments/comment_sequence_identity.rs"]
mod comment_sequence_identity;
#[path = "signature_results.rs"]
mod issue_830_signature_results;
#[path = "variadic_calls/semantics.rs"]
mod postgresql18_variadic_calls_wave7;

include!("../comments/routine_objects/metadata.rs");

include!("../comments/routine_objects/restart.rs");

include!("../comments/routine_objects/semantics.rs");
