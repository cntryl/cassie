use super::{
    Argon2, CassieError, OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
};

pub(super) fn hash_password(password: &str) -> Result<String, CassieError> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| CassieError::Execution(format!("failed to hash role password: {error}")))
}

pub(super) fn verify_password(hash: &str, password: &str) -> Result<bool, CassieError> {
    let parsed = PasswordHash::new(hash)
        .map_err(|error| CassieError::Execution(format!("invalid password hash: {error}")))?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}
