use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use cassie::app::{Cassie, CassieError};
use cassie::config::CassieRuntimeConfig;
use reqwest::{Client, StatusCode};
use tokio::io::AsyncWriteExt;
use tokio::sync::Notify;
use tokio::task::JoinHandle;

use crate::support_authentication_redaction::{
    assert_secret_free, emit_capture_positive_controls, enter_owned_child, retain_private_canaries,
    TraceCapture,
};
use crate::support_pgwire as wire;

const COMPLETION: &str = "AUTH03 owned child completed: wire3 rest3 capture2 cleanup1";
const DEADLINE: Duration = Duration::from_secs(10);

struct WireObservation {
    challenge: (u8, Vec<u8>),
    reply: (u8, Vec<u8>),
}

struct RestObservation {
    status: StatusCode,
    body: Vec<u8>,
    cookie: bool,
    retry_after: Option<String>,
}

struct Observations {
    wire: Vec<WireObservation>,
    rest: Vec<RestObservation>,
    servers_stopped: bool,
}

async fn reserve_addresses() -> (SocketAddr, SocketAddr) {
    // Hold both reservations together so the two endpoint candidates are distinct.
    let wire = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("reserve wire address");
    let rest = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("reserve REST address");
    (
        wire.local_addr().expect("wire address"),
        rest.local_addr().expect("REST address"),
    )
}

async fn wait_for_listener(address: SocketAddr) {
    tokio::time::timeout(DEADLINE, async {
        loop {
            if let Ok(stream) = tokio::net::TcpStream::connect(address).await {
                drop(stream);
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("owned listener readiness");
}

async fn wire_attempt(address: SocketAddr, user: &str, password: &str) -> WireObservation {
    tokio::time::timeout(DEADLINE, async {
        let mut socket = tokio::net::TcpStream::connect(address)
            .await
            .expect("wire connection");
        socket
            .write_all(&wire::startup_frame(user, "postgres"))
            .await
            .expect("startup write");
        let challenge = wire::read_wire_frame(&mut socket).await;
        socket
            .write_all(&wire::password_message(password))
            .await
            .expect("password write");
        let reply = wire::read_wire_frame(&mut socket).await;
        drop(socket);
        WireObservation { challenge, reply }
    })
    .await
    .expect("wire authentication deadline")
}

async fn rest_attempt(
    client: &Client,
    address: SocketAddr,
    user: &str,
    password: &str,
) -> RestObservation {
    let response = client
        .post(format!("http://{address}/api/v1/auth/login"))
        .json(&serde_json::json!({"username": user, "password": password}))
        .send()
        .await
        .expect("REST login response");
    let status = response.status();
    let cookie = response.headers().contains_key("set-cookie");
    let retry_after = response
        .headers()
        .get("retry-after")
        .map(|value| value.to_str().expect("retry header").to_string());
    let body = response.bytes().await.expect("REST login body").to_vec();
    RestObservation {
        status,
        body,
        cookie,
        retry_after,
    }
}

async fn stop_server(
    mut server: JoinHandle<Result<(), CassieError>>,
    shutdown: Arc<Notify>,
) -> bool {
    shutdown.notify_one();
    match tokio::time::timeout(DEADLINE, &mut server).await {
        Ok(Ok(Ok(()))) => true,
        Ok(_) => false,
        Err(_) => {
            server.abort();
            let _ = server.await;
            false
        }
    }
}

async fn observe(cassie: &Cassie, config: CassieRuntimeConfig, password: &str) -> Observations {
    let (wire_address, rest_address) = reserve_addresses().await;
    let wire_shutdown = Arc::new(Notify::new());
    let rest_shutdown = Arc::new(Notify::new());
    let wire_server = tokio::spawn(cassie::pgwire::server::run_with_shutdown(
        wire_address.to_string(),
        Arc::new(cassie.clone()),
        config,
        wire_shutdown.clone(),
    ));
    let rest_server = tokio::spawn(cassie::rest::router::run_with_shutdown(
        rest_address.to_string(),
        cassie.clone(),
        rest_shutdown.clone(),
    ));
    wait_for_listener(wire_address).await;
    wait_for_listener(rest_address).await;
    emit_capture_positive_controls().await;
    let client = Client::builder()
        .timeout(DEADLINE)
        .build()
        .expect("bounded HTTP client");

    // One-token user buckets make the second adjacent known-user attempt throttle.
    // Distinct known/unknown names keep wire and REST bucket state independent.
    let wire = vec![
        wire_attempt(wire_address, "wire_redaction_reader", password).await,
        wire_attempt(wire_address, "wire_redaction_reader", password).await,
        wire_attempt(wire_address, "wire_redaction_unknown", password).await,
    ];
    let rest = vec![
        rest_attempt(&client, rest_address, "rest_redaction_reader", password).await,
        rest_attempt(&client, rest_address, "rest_redaction_reader", password).await,
        rest_attempt(&client, rest_address, "rest_redaction_unknown", password).await,
    ];
    drop(client);
    let wire_stopped = stop_server(wire_server, wire_shutdown).await;
    let rest_stopped = stop_server(rest_server, rest_shutdown).await;
    Observations {
        wire,
        rest,
        servers_stopped: wire_stopped && rest_stopped,
    }
}

fn assert_observations(observed: &Observations, canaries: &[String]) {
    assert!(
        observed.servers_stopped,
        "both owned servers must stop cleanly"
    );
    assert_eq!(observed.wire.len(), 3);
    for observation in &observed.wire {
        assert_secret_free(&observation.challenge.1, canaries);
        assert_secret_free(&observation.reply.1, canaries);
        assert_eq!(observation.challenge.0, b'R');
        assert!(
            observation.challenge.1 == [0, 0, 0, 3],
            "actual cleartext password challenge"
        );
        assert_eq!(observation.reply.0, b'E');
        let fields = wire::parse_error_fields(&observation.reply.1);
        for (field, expected) in [
            ('S', "FATAL"),
            ('C', "28000"),
            ('M', "authentication failed"),
        ] {
            assert!(
                fields
                    .iter()
                    .find(|(kind, _)| *kind == field)
                    .is_some_and(|(_, value)| value == expected),
                "generic wire authentication error field"
            );
        }
    }
    assert!(
        observed.wire[0].reply == observed.wire[1].reply
            && observed.wire[0].reply == observed.wire[2].reply,
        "known, unknown and throttled wire errors equal"
    );
    assert_eq!(observed.rest.len(), 3);
    for response in &observed.rest {
        assert_secret_free(&response.body, canaries);
        assert!(!response.cookie, "failed login must not issue a cookie");
        if let Some(header) = &response.retry_after {
            assert_secret_free(header.as_bytes(), canaries);
        }
    }
    assert_eq!(observed.rest[0].status, StatusCode::UNAUTHORIZED);
    assert_eq!(observed.rest[2].status, StatusCode::UNAUTHORIZED);
    assert!(
        observed.rest[0].body == observed.rest[2].body,
        "invalid known and unknown REST bodies equal"
    );
    let invalid: serde_json::Value =
        serde_json::from_slice(&observed.rest[0].body).expect("invalid-login JSON");
    assert!(
        invalid == serde_json::json!({"error": "unauthorized"}),
        "generic invalid-login body"
    );
    assert_eq!(observed.rest[1].status, StatusCode::TOO_MANY_REQUESTS);
    let throttle: serde_json::Value =
        serde_json::from_slice(&observed.rest[1].body).expect("throttled-login JSON");
    assert!(
        throttle == serde_json::json!({"error": "authentication rate limit exceeded"}),
        "preserve current generic throttle taxonomy"
    );
    assert!(
        observed.rest[1].retry_after.as_deref() == Some("60"),
        "current throttle retry header"
    );
}

#[test]
fn should_redact_credentials_across_network_authentication_failures() {
    // Arrange
    if !enter_owned_child(
        "authentication_redaction::should_redact_credentials_across_network_authentication_failures",
        "CASSIE_AUTH03_GLOBAL_TRACE_CHILD", COMPLETION,
    ) {
        return;
    }
    let capture = TraceCapture::install_in_owned_child();
    let actual_password = format!("auth03-actual-{}", uuid::Uuid::new_v4());
    let wrong_password = format!("auth03-invalid-{}", uuid::Uuid::new_v4());
    let config = CassieRuntimeConfig {
        password: format!("auth03-bootstrap-{}", uuid::Uuid::new_v4()),
        auth_user_attempts_per_minute: 1,
        auth_ip_attempts_per_minute: 100,
        ..CassieRuntimeConfig::default()
    };
    let mut canaries = vec![
        actual_password.clone(),
        wrong_password.clone(),
        config.password.clone(),
    ];
    let cassie = Cassie::new_in_memory_with_config(config.clone()).expect("Cassie");
    cassie.startup().expect("startup");
    for name in ["wire_redaction_reader", "rest_redaction_reader", "root"] {
        if name != "root" {
            cassie
                .create_role(name, true, Some(actual_password.clone()), false)
                .expect("known role");
        }
        let Ok(Some(role)) = cassie.midge.get_role(name) else {
            panic!("actual persisted role required");
        };
        let hash = role
            .password_hash
            .expect("actual persisted credential hash");
        assert!(hash.starts_with("$argon2"), "actual stored hash canary");
        canaries.push(hash);
    }
    retain_private_canaries(&canaries);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    // Act
    let observed = runtime.block_on(observe(&cassie, config, &wrong_password));
    drop(runtime);
    cassie.shutdown();
    drop(cassie);

    // Assert
    assert_observations(&observed, &canaries);
    capture.assert_redacted_with_both_domains(&canaries);
    println!("\n{COMPLETION}");
}
