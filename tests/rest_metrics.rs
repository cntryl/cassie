use cassie::app::Cassie;
use uuid::Uuid;

fn with_fallback() {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
}

fn data_dir(label: &str) -> String {
    let mut path = std::env::temp_dir();
    path.push(format!("cassie-rest-metrics-{}-{}", label, Uuid::new_v4()));
    path.to_string_lossy().to_string()
}

#[test]
fn should_record_rest_request_metrics_for_http_routes() {
    // Arrange
    with_fallback();
    let path = data_dir("http_routes");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener address");
        drop(listener);

        let server = tokio::spawn(cassie::rest::router::run(addr.to_string(), cassie.clone()));

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let client = reqwest::Client::new();

        // Act
        let health = client
            .get(format!("http://{addr}/health"))
            .send()
            .await
            .expect("health request");
        assert!(health.status().is_success());

        let metrics = client
            .get(format!("http://{addr}/metrics"))
            .send()
            .await
            .expect("metrics request")
            .json::<serde_json::Value>()
            .await
            .expect("metrics json");

        // Assert
        assert!(
            metrics["rest"]["requests_total"]
                .as_u64()
                .unwrap_or_default()
                >= 1,
            "rest request count should be recorded"
        );
        assert!(
            metrics["rest"]["by_method"]["GET"]
                .as_u64()
                .unwrap_or_default()
                >= 1,
            "rest method counter should be recorded"
        );
        assert!(
            metrics["rest"]["by_route"]["/health"]
                .as_u64()
                .unwrap_or_default()
                >= 1,
            "rest route counter should be recorded"
        );
        assert!(
            metrics["rest"]["by_status_class"]["2xx"]
                .as_u64()
                .unwrap_or_default()
                >= 1,
            "rest status-class counter should be recorded"
        );

        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}
