use super::*;

#[path = "catalog_identity_lookups.rs"]
mod catalog_identity_lookups;
#[path = "../comments/comment_catalog_coverage.rs"]
mod comment_catalog_coverage;
#[path = "../comments/comment_owner_lifecycle.rs"]
mod comment_owner_lifecycle;
#[path = "../comments/comment_sequence_identity.rs"]
mod comment_sequence_identity;
#[path = "../security/default_privileges.rs"]
mod default_privileges;
#[path = "signature_results.rs"]
mod issue_830_signature_results;
#[path = "../security/ownership_privileges.rs"]
mod ownership_privileges;
#[path = "variadic_calls/semantics.rs"]
mod postgresql18_variadic_calls_wave7;
#[path = "../security/role_membership_admin_option/semantics.rs"]
mod role_membership_admin_option_wave7;
#[path = "../casts/sql_defined_builtin_casts.rs"]
mod sql_defined_builtin_casts;

include!("../comments/routine_objects/metadata.rs");

include!("../comments/routine_objects/restart.rs");

include!("../comments/routine_objects/semantics.rs");

include!("../security/routine_objects/semantics.rs");
