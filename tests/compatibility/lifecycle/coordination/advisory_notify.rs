use super::*;

#[path = "advisory_lock_key_space_identity/semantics.rs"]
mod advisory_lock_key_space_identity;
#[path = "notification_deduplication_key/transactions.rs"]
mod notification_deduplication_key;

include!("advisory_notify/cancellation.rs");

include!("advisory_notify/errors.rs");

include!("advisory_notify/metadata.rs");

include!("advisory_notify/semantics.rs");

include!("advisory_notify/transactions.rs");
