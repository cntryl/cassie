use std::path::Path;

pub(crate) fn client_trusting_test_certificate(certificate_path: &Path) -> reqwest::Client {
    let certificate_pem = std::fs::read(certificate_path).expect("read test TLS certificate");
    let certificate =
        reqwest::Certificate::from_pem(&certificate_pem).expect("parse test TLS certificate");
    reqwest::Client::builder()
        .add_root_certificate(certificate)
        .build()
        .expect("build test TLS client")
}
