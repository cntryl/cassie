use super::*;

fn lifecycle_wave_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread Tokio runtime")
}

include!("comments/lifecycle/transactions.rs");

include!("extensions/lifecycle/semantics.rs");

include!("large_objects/lifecycle/resources.rs");

include!("security/privileges/lifecycle/semantics.rs");
