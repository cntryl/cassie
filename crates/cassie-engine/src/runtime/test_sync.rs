//! Compatibility re-export for Engine's test synchronization helpers.

pub use cassie_core::test_sync::{
    join_test_thread, recv_test, wait_for_test_barrier, TestBarrierCleanup,
};
