//! Finite independent relational truth tables on the selected merged contracts.

#[path = "support/pgwire.rs"]
mod support_pgwire;
#[path = "support/relational_qualification.rs"]
mod support_relational_qualification;
#[path = "support/temp_dirs.rs"]
mod support_temp_dirs;

#[path = "relational_qualification/primary.rs"]
mod primary;

#[path = "relational_qualification/positive_variants.rs"]
mod positive_variants;

#[path = "relational_qualification/frames.rs"]
mod frames;

#[path = "relational_qualification/negative.rs"]
mod negative;

#[path = "relational_qualification/paths.rs"]
mod paths;
#[path = "support/relational_paths.rs"]
mod support_relational_paths;

#[path = "relational_qualification/pagination.rs"]
mod pagination;

#[path = "relational_qualification/homogeneous_join.rs"]
mod homogeneous_join;

#[path = "relational_qualification/joined_correlation.rs"]
mod joined_correlation;

#[path = "relational_qualification/ancestor_scope.rs"]
mod ancestor_scope;

#[path = "relational_qualification/exists_having.rs"]
mod exists_having;

#[path = "relational_qualification/exists_order.rs"]
mod exists_order;

#[path = "relational_qualification/exists_window.rs"]
mod exists_window;

#[path = "relational_qualification/exists_returning.rs"]
mod exists_returning;

#[path = "relational_qualification/join_on.rs"]
mod join_on;
