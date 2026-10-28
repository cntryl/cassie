//! PostgreSQL lock and safe-snapshot blocker functions (#817).

use std::time::{Duration, Instant};

use crate::support_catalog_sessions as fixture;
