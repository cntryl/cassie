#[path = "support/sql.rs"]
mod support_sql;
#[path = "support/temp_dirs.rs"]
mod support_temp_dirs;

use cassie::app::Cassie;
use cassie::app::CassieError;
use cassie::types::Value;

use support_sql::{data_dir, use_local_storage};

include!("sql_contextual_literals/contracts/errors.rs");

include!("sql_contextual_literals/contracts/semantics.rs");
