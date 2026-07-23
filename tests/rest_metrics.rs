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

async fn login_cookie(client: &reqwest::Client, addr: std::net::SocketAddr) -> String {
    client
        .post(format!("http://{addr}/api/v1/auth/login"))
        .json(&serde_json::json!({
            "username": "postgres",
            "password": "postgres"
        }))
        .send()
        .await
        .expect("login request")
        .headers()
        .get("set-cookie")
        .expect("session cookie")
        .to_str()
        .expect("session cookie value")
        .split(';')
        .next()
        .expect("session cookie pair")
        .to_string()
}

fn assert_rest_metrics(metrics: &serde_json::Value) {
    assert!(
        metrics["rest"]["requests_total"]
            .as_u64()
            .unwrap_or_default()
            >= 1
    );
    assert!(
        metrics["rest"]["by_method"]["GET"]
            .as_u64()
            .unwrap_or_default()
            >= 1
    );
    assert!(
        metrics["rest"]["by_route"]["/health"]
            .as_u64()
            .unwrap_or_default()
            >= 1
    );
    assert!(
        metrics["rest"]["by_route"]["/liveness"]
            .as_u64()
            .unwrap_or_default()
            >= 1
    );
    assert!(
        metrics["rest"]["by_status_class"]["2xx"]
            .as_u64()
            .unwrap_or_default()
            >= 1
    );
    let routes = metrics["rest"]["by_route"]
        .as_object()
        .expect("route metrics");
    assert!(routes.len() <= 257);
    assert!(routes["<other>"].as_u64().unwrap_or_default() > 0);
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
        cassie.startup().unwrap();

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

        let liveness = client
            .get(format!("http://{addr}/liveness"))
            .send()
            .await
            .expect("liveness request");
        assert!(liveness.status().is_success());
        for index in 0..300 {
            client
                .get(format!("http://{addr}/unmatched-{index}"))
                .send()
                .await
                .expect("unmatched request");
        }

        let unauthenticated_metrics = client
            .get(format!("http://{addr}/metrics"))
            .send()
            .await
            .expect("unauthenticated metrics request");
        let cookie = login_cookie(&client, addr).await;
        let metrics = client
            .get(format!("http://{addr}/metrics"))
            .header("cookie", cookie)
            .send()
            .await
            .expect("authenticated metrics request")
            .json::<serde_json::Value>()
            .await
            .expect("metrics json");

        // Assert
        assert_eq!(
            unauthenticated_metrics.status(),
            reqwest::StatusCode::UNAUTHORIZED
        );
        assert_rest_metrics(&metrics);

        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}
