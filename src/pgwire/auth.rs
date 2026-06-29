use crate::app::CassieError;

/// # Errors
///
/// Returns an error when validation, storage, or execution fails.
pub fn validate_user_password(
    configured_user: &str,
    configured_password: &str,
    user: &str,
    password: Option<&str>,
) -> Result<(), CassieError> {
    if configured_password.is_empty()
        || (user == configured_user && password.unwrap_or("") == configured_password)
    {
        Ok(())
    } else {
        Err(CassieError::Unauthorized)
    }
}
