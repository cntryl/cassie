//! Selected transaction state laws exercised through independent wire sessions.

use super::support_pgwire as wire;
use super::support_pgwire_transaction_boundary::{run_boundary, run_cycle, run_script, Prefix};

include!("../protocol/transaction_boundaries/semantics.rs");

include!("../joins/transaction_boundaries/errors.rs");

include!("../joins/transaction_boundaries/semantics.rs");

include!("../protocol/transaction_boundaries/errors.rs");

include!("../ddl/transaction_boundaries/errors.rs");

include!("../protocol/transaction_boundaries/transactions.rs");

include!("../portals/transaction_boundaries/semantics.rs");

include!("../protocol/transaction_boundaries/nulls.rs");

fn copy_cycle() -> Vec<Vec<u8>> {
    vec![
        wire::simple_query_frame("COPY wire_tx (n) FROM STDIN WITH (FORMAT csv)"),
        wire::copy_data_frame(b"2\n"),
        wire::copy_done_frame(),
    ]
}

include!("../copy/transaction_boundaries/semantics.rs");

include!("../copy/transaction_boundaries/transactions.rs");

include!("../copy/transaction_boundaries/errors.rs");

include!("transaction_boundaries/errors.rs");

include!("transaction_boundaries/resources.rs");

include!("transaction_boundaries/semantics.rs");
