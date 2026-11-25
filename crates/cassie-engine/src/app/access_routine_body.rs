//! Authorization of the stored body of a user function a statement calls.
//!
//! A user function runs with the caller's privileges, so its body may touch
//! only what the caller could touch directly: a body that calls `nextval` or
//! reads a table is refused for a role that could not issue that call itself.
use super::{CassieError, CassieSession};
use std::cell::RefCell;

thread_local! {
    /// Functions whose bodies are being authorized on this thread; a function
    /// that reaches itself again is already under check.
    static ACTIVE_ROUTINES: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

struct ActiveRoutine;

impl ActiveRoutine {
    /// Marks `name` active, or returns `None` when it already is.
    fn enter(name: &str) -> Option<Self> {
        ACTIVE_ROUTINES.with(|active| {
            let mut active = active.borrow_mut();
            if active.iter().any(|entry| entry == name) {
                None
            } else {
                active.push(name.to_owned());
                Some(Self)
            }
        })
    }
}

impl Drop for ActiveRoutine {
    fn drop(&mut self) {
        ACTIVE_ROUTINES.with(|active| {
            active.borrow_mut().pop();
        });
    }
}

/// Sequence functions are refused outright: sequences have no grants of their
/// own, so only an administrator may use them.
fn reject_sequence_function(name: &str) -> Result<(), CassieError> {
    let function_id = crate::sql::functions::function(name).map(|function| function.id);
    if matches!(
        function_id,
        Some(
            crate::sql::functions::FunctionId::Nextval
                | crate::sql::functions::FunctionId::Currval
                | crate::sql::functions::FunctionId::Setval
                | crate::sql::functions::FunctionId::Lastval
                | crate::sql::functions::FunctionId::PgGetSerialSequence
        )
    ) {
        Err(CassieError::InsufficientPrivilege)
    } else {
        Ok(())
    }
}

impl crate::app::Cassie {
    /// Authorizes a call to the function `name`: sequence functions are refused,
    /// and a user function's stored body must be allowed for the caller too.
    pub(super) fn authorize_function_call(
        &self,
        session: &CassieSession,
        context: &crate::sql::binder::BindingContext,
        name: &str,
    ) -> Result<(), CassieError> {
        reject_sequence_function(name)?;
        self.authorize_user_routine_body(session, context, name)
    }

    fn authorize_user_routine_body(
        &self,
        session: &CassieSession,
        context: &crate::sql::binder::BindingContext,
        name: &str,
    ) -> Result<(), CassieError> {
        let lookup = name.to_ascii_lowercase();
        if lookup != "width_bucket" && crate::sql::functions::function(name).is_some() {
            return Ok(());
        }
        let Some(routine) = self
            .user_functions_for_session(Some(session))
            .remove(&lookup)
        else {
            return Ok(());
        };
        let Some(_active) = ActiveRoutine::enter(&routine.name.to_ascii_lowercase()) else {
            return Ok(());
        };
        // An unparsable body fails at call time, before it can do anything.
        let Ok(body) = crate::sql::parser::parse_expression(&routine.body) else {
            return Ok(());
        };
        self.authorize_expression(session, context, &body)
    }
}
