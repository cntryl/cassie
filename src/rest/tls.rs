use std::fs::File;
use std::io::{BufReader, ErrorKind};
use std::sync::Arc;

use rustls::ServerConfig;

use crate::app::CassieError;

pub(crate) fn load_server_config(
    certificate_file: Option<&str>,
    key_file: Option<&str>,
) -> Result<Option<Arc<ServerConfig>>, CassieError> {
    let (Some(certificate_file), Some(key_file)) = (certificate_file, key_file) else {
        return Ok(None);
    };

    let certificates = read_certificates(certificate_file)?;
    let private_key = read_private_key(key_file)?;
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certificates, private_key)
        .map_err(|error| CassieError::Execution(format!("invalid REST TLS identity: {error}")))?;
    Ok(Some(Arc::new(config)))
}

fn read_certificates(
    path: &str,
) -> Result<Vec<rustls::pki_types::CertificateDer<'static>>, CassieError> {
    let file = File::open(path).map_err(|error| tls_file_error("certificate", path, &error))?;
    rustls_pemfile::certs(&mut BufReader::new(file))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            CassieError::Execution(format!("invalid REST TLS certificate '{path}': {error}"))
        })
}

fn read_private_key(path: &str) -> Result<rustls::pki_types::PrivateKeyDer<'static>, CassieError> {
    let file = File::open(path).map_err(|error| tls_file_error("private key", path, &error))?;
    rustls_pemfile::private_key(&mut BufReader::new(file))
        .map_err(|error| {
            CassieError::Execution(format!("invalid REST TLS private key '{path}': {error}"))
        })?
        .ok_or_else(|| {
            CassieError::Execution(format!(
                "REST TLS private key '{path}' contains no private key"
            ))
        })
}

fn tls_file_error(kind: &str, path: &str, error: &std::io::Error) -> CassieError {
    let detail = if error.kind() == ErrorKind::NotFound {
        "file not found"
    } else {
        "file could not be read"
    };
    CassieError::Execution(format!("REST TLS {kind} '{path}': {detail}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::load_server_config;

    fn temporary_paths() -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
        let directory =
            std::env::temp_dir().join(format!("cassie-rest-tls-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).expect("TLS test directory");
        (
            directory.clone(),
            directory.join("cert.pem"),
            directory.join("key.pem"),
        )
    }

    #[test]
    fn should_load_valid_rest_tls_identity() {
        // Arrange
        let (directory, certificate, key) = temporary_paths();
        let identity = rcgen::generate_simple_self_signed(vec!["localhost".to_string()])
            .expect("certificate identity");
        std::fs::write(&certificate, identity.cert.pem()).expect("certificate fixture");
        std::fs::write(&key, identity.key_pair.serialize_pem()).expect("key fixture");

        // Act
        let config = load_server_config(
            Some(certificate.to_str().expect("certificate path")),
            Some(key.to_str().expect("key path")),
        )
        .expect("valid identity should load");

        // Assert
        assert!(config.is_some());
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn should_reject_missing_rest_tls_certificate() {
        // Arrange
        let certificate = "/tmp/cassie-missing-rest-cert.pem";
        let key = "/tmp/cassie-missing-rest-key.pem";

        // Act
        let error = load_server_config(Some(certificate), Some(key))
            .expect_err("missing certificate should fail closed");

        // Assert
        assert!(error.to_string().contains("REST TLS"));
        assert!(error.to_string().contains("file not found"));
    }

    #[test]
    fn should_reject_invalid_rest_tls_certificate() {
        // Arrange
        let (directory, certificate, key) = temporary_paths();
        std::fs::write(&certificate, b"not a certificate").expect("certificate fixture");
        std::fs::write(&key, b"not a key").expect("key fixture");

        // Act
        let error = load_server_config(
            Some(certificate.to_str().expect("certificate path")),
            Some(key.to_str().expect("key path")),
        )
        .expect_err("invalid certificate should fail closed");

        // Assert
        assert!(error.to_string().contains("REST TLS"));
        let _ = std::fs::remove_dir_all(directory);
    }
}
