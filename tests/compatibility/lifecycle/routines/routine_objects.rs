use super::*;

#[path = "catalog_identity_lookups.rs"]
mod catalog_identity_lookups;
#[path = "../comments/comment_catalog_coverage.rs"]
mod comment_catalog_coverage;

include!("../comments/routine_objects/restart.rs");

include!("../comments/routine_objects/semantics.rs");
