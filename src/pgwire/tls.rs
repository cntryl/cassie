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
        .map_err(|error| CassieError::Execution(format!("invalid pgwire TLS identity: {error}")))?;
    Ok(Some(Arc::new(config)))
}

fn read_certificates(
    path: &str,
) -> Result<Vec<rustls::pki_types::CertificateDer<'static>>, CassieError> {
    let file = File::open(path).map_err(|error| tls_file_error("certificate", path, &error))?;
    let certificates = rustls_pemfile::certs(&mut BufReader::new(file))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            CassieError::Execution(format!("invalid pgwire TLS certificate '{path}': {error}"))
        })?;
    if certificates.is_empty() {
        return Err(CassieError::Execution(format!(
            "pgwire TLS certificate '{path}' contains no certificates"
        )));
    }
    Ok(certificates)
}

fn read_private_key(path: &str) -> Result<rustls::pki_types::PrivateKeyDer<'static>, CassieError> {
    let file = File::open(path).map_err(|error| tls_file_error("private key", path, &error))?;
    rustls_pemfile::private_key(&mut BufReader::new(file))
        .map_err(|error| {
            CassieError::Execution(format!("invalid pgwire TLS private key '{path}': {error}"))
        })?
        .ok_or_else(|| {
            CassieError::Execution(format!(
                "pgwire TLS private key '{path}' contains no private key"
            ))
        })
}

fn tls_file_error(kind: &str, path: &str, error: &std::io::Error) -> CassieError {
    let detail = if error.kind() == ErrorKind::NotFound {
        "file not found"
    } else {
        "file could not be read"
    };
    CassieError::Execution(format!("pgwire TLS {kind} '{path}': {detail}: {error}"))
}
