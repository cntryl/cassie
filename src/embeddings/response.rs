use std::io::Read;

use reqwest::blocking::Response;
use reqwest::StatusCode;

use super::EmbeddingError;

const MAX_ERROR_EXCERPT_BYTES: usize = 1024;

#[derive(Debug, thiserror::Error)]
pub(crate) enum ResponseReadError {
    #[error(transparent)]
    Network(#[from] reqwest::Error),
    #[error("provider response read failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("provider response exceeds {limit_bytes} bytes")]
    TooLarge { limit_bytes: usize },
}

impl ResponseReadError {
    pub(crate) fn is_timeout(&self) -> bool {
        matches!(self, Self::Network(error) if error.is_timeout())
    }

    pub(crate) fn is_connect(&self) -> bool {
        matches!(self, Self::Network(error) if error.is_connect())
    }

    pub(crate) fn into_embedding_error(self, provider: &str) -> EmbeddingError {
        match self {
            Self::TooLarge { limit_bytes } => EmbeddingError::ResponseTooLarge {
                provider: provider.to_string(),
                limit_bytes,
            },
            other => EmbeddingError::RequestError(other.to_string()),
        }
    }
}

pub(crate) fn read_response(
    mut response: Response,
    max_response_bytes: usize,
) -> Result<(StatusCode, String), ResponseReadError> {
    let max_response_bytes = max_response_bytes.max(1);
    if response
        .content_length()
        .is_some_and(|length| length > max_response_bytes as u64)
    {
        return Err(ResponseReadError::TooLarge {
            limit_bytes: max_response_bytes,
        });
    }

    let status = response.status();
    let read_limit = u64::try_from(max_response_bytes)
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    let mut body = Vec::with_capacity(
        response
            .content_length()
            .and_then(|length| usize::try_from(length).ok())
            .unwrap_or(0)
            .min(max_response_bytes),
    );
    response.by_ref().take(read_limit).read_to_end(&mut body)?;
    if body.len() > max_response_bytes {
        return Err(ResponseReadError::TooLarge {
            limit_bytes: max_response_bytes,
        });
    }

    let body = String::from_utf8_lossy(&body);
    if status.is_success() {
        Ok((status, body.into_owned()))
    } else {
        Ok((status, sanitize_error_excerpt(&body)))
    }
}

fn sanitize_error_excerpt(body: &str) -> String {
    let mut excerpt = String::new();
    for character in body.chars().filter(|character| !character.is_control()) {
        if excerpt.len().saturating_add(character.len_utf8()) > MAX_ERROR_EXCERPT_BYTES {
            break;
        }
        excerpt.push(character);
    }
    excerpt
}

#[cfg(test)]
mod tests {
    use super::sanitize_error_excerpt;

    #[test]
    fn should_cap_and_strip_controls_from_provider_error_excerpts() {
        // Arrange
        let body = format!("prefix\u{0}\n{}", "x".repeat(2_048));

        // Act
        let excerpt = sanitize_error_excerpt(&body);

        // Assert
        assert!(excerpt.len() <= 1_024);
        assert!(!excerpt.chars().any(char::is_control));
        assert!(!excerpt.ends_with(&"x".repeat(1_024)));
    }
}
