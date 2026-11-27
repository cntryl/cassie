pub(crate) mod body;
pub mod collections;
pub mod consistency;
pub(crate) mod databases;
pub mod documents;
pub mod health;
pub mod indexes;
mod oidc;
pub mod query;
pub(crate) mod query_operations;
#[doc(hidden)]
pub use query_operations::is_active_for_test;
pub mod router;
pub(crate) mod scope;
pub mod search;
pub(crate) mod sessions;
pub(crate) mod static_files;
pub(crate) mod tls;
