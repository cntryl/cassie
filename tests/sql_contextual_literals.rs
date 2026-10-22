#[path = "support/sql.rs"]
mod support_sql;
#[path = "support/temp_dirs.rs"]
mod support_temp_dirs;

use cassie::app::CassieError;
use cassie::types::Value;

include!("sql_contextual_literals/contracts/errors.rs");

include!("sql_contextual_literals/contracts/semantics.rs");
