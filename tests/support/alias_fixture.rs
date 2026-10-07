//! The reader identity used by the finite alias authorization fixture.
use super::support_sql_fixture::SqlFixture;
use cassie::app::CassieSession;

/// Creates and authenticates the alias fixture reader at the call site.
///
/// # Panics
/// Panics when fixture role creation or authentication fails.
pub fn reader(fixture: &SqlFixture) -> CassieSession {
    fixture
        .cassie
        .create_role("alias_reader", true, Some("fixture-password".into()), false)
        .expect("reader role");
    fixture
        .cassie
        .authenticate_role("alias_reader", Some("fixture-password"), None)
        .expect("authenticate reader")
}
