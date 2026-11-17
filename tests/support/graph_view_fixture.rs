//! Started graph-view lifecycle fixtures with an explicit caller namespace.

use crate::support_sql_fixture::{sql_fixture, SqlFixture};

/// Starts an in-memory instance before creating the session and scoped objects.
///
/// # Panics
///
/// Panics when startup, caller namespace selection or a setup statement fails.
#[must_use]
pub fn started_shadow_fixture(label: &str, setup: &[&str]) -> SqlFixture {
    let mut fixture = sql_fixture(label, &[]);
    fixture.cassie.startup().expect("start graph-view fixture");
    fixture.session = fixture.cassie.create_session("tester", None);
    fixture
        .execute("SET search_path TO public")
        .expect("explicit public caller search path");
    for sql in setup {
        fixture
            .execute(sql)
            .expect("seed scoped graph-view fixture");
    }
    fixture
}
