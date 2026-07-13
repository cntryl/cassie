use cassie::app::{Cassie, CassieError, ProjectionManifestExportOptions};
use cassie::catalog::canonical_relation_name;
use cassie::config::CassieRuntimeConfig;
use cassie::types::Value;
use uuid::Uuid;

#[path = "support/sql.rs"]
mod support;
use support::*;

fn canonical_collection(name: &str) -> String {
    canonical_relation_name("postgres", "public", name)
}

fn create_manifest_source(
    label: &str,
    title: &str,
) -> (Cassie, String, String, ProjectionManifestExportOptions) {
    with_fallback();
    let dir = data_dir(label);
    let cassie = Cassie::new_with_data_dir(&dir).expect("cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE consistency_docs (title TEXT, body TEXT, embedding VECTOR(2))",
            vec![],
        )
        .expect("create table");
    let projection = canonical_collection("consistency_docs");
    cassie
        .midge
        .put_document(
            &projection,
            Some("doc-2".to_string()),
            serde_json::json!({
                "title": title,
                "body": "sensitive body text",
                "embedding": [0.25, 0.75]
            }),
        )
        .expect("insert doc-2");
    cassie
        .midge
        .put_document(
            &projection,
            Some("doc-1".to_string()),
            serde_json::json!({
                "title": "alpha",
                "body": "secret password bind value",
                "embedding": [1.0, 0.0]
            }),
        )
        .expect("insert doc-1");

    let mut options = ProjectionManifestExportOptions::for_instance(label);
    options.generated_ms = Some(4_000_000_000_000);
    options.ttl_ms = Some(86_400_000);
    options.include_row_hashes = true;
    (cassie, dir, projection, options)
}

async fn spawn_rest_server(
    cassie: Cassie,
) -> (String, tokio::task::JoinHandle<Result<(), CassieError>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind listener");
    let addr = listener.local_addr().expect("listener address");
    drop(listener);
    let server = tokio::spawn(cassie::rest::router::run(addr.to_string(), cassie));
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    (format!("http://{addr}"), server)
}

async fn login_cookie(client: &reqwest::Client, base_url: &str) -> String {
    client
        .post(format!("{base_url}/api/v1/auth/login"))
        .json(&serde_json::json!({
            "username": "sa",
            "password": "topsecret"
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

#[test]
fn should_export_projection_verification_manifest_with_canonical_ordering() {
    // Arrange
    let (cassie, _dir, projection, options) = create_manifest_source("instance-a", "bravo");

    // Act
    let first = cassie
        .export_projection_verification_manifest(&projection, options.clone())
        .expect("first manifest");
    let second = cassie
        .export_projection_verification_manifest(&projection, options)
        .expect("second manifest");

    // Assert
    assert_eq!(first.manifest_version, 1);
    assert_eq!(first.instance_id, "instance-a");
    assert_eq!(first.projection_id, projection);
    assert_eq!(first.generated_ms, 4_000_000_000_000);
    assert_eq!(first.manifest_digest, second.manifest_digest);
    assert!(first
        .ranges
        .windows(2)
        .all(|pair| pair[0].range_id <= pair[1].range_id));
    assert!(first
        .row_hashes
        .windows(2)
        .all(|pair| pair[0].row_id <= pair[1].row_id));
}

#[test]
fn should_compare_equal_manifests_as_consistent() {
    // Arrange
    let (left, _left_dir, projection, left_options) = create_manifest_source("instance-a", "bravo");
    let (right, _right_dir, _, right_options) = create_manifest_source("instance-b", "bravo");
    let left_manifest = left
        .export_projection_verification_manifest(&projection, left_options)
        .expect("left manifest");
    let right_manifest = right
        .export_projection_verification_manifest(&projection, right_options)
        .expect("right manifest");

    // Act
    let report = left
        .compare_projection_verification_manifests(vec![right_manifest, left_manifest])
        .expect("compare manifests");

    // Assert
    assert_eq!(report.state, "consistent");
    assert_eq!(report.manifest_count, 2);
    assert_eq!(
        report.instance_ids,
        vec!["instance-a".to_string(), "instance-b".to_string()]
    );
    assert_eq!(report.mismatch_count, 0);
}

#[test]
fn should_report_row_level_divergence_when_hashes_are_available() {
    // Arrange
    let (left, _left_dir, projection, left_options) = create_manifest_source("instance-a", "bravo");
    let (right, _right_dir, _, right_options) = create_manifest_source("instance-b", "charlie");
    let left_manifest = left
        .export_projection_verification_manifest(&projection, left_options)
        .expect("left manifest");
    let right_manifest = right
        .export_projection_verification_manifest(&projection, right_options)
        .expect("right manifest");

    // Act
    let report = left
        .compare_projection_verification_manifests(vec![left_manifest, right_manifest])
        .expect("compare manifests");

    // Assert
    assert_eq!(report.state, "divergent");
    assert_eq!(report.mismatch_count, 1);
    assert_eq!(report.divergent_range_count, 1);
    assert_eq!(report.divergent_row_count, 1);
    assert!(report.diagnostic_sample.contains(&"row:doc-2".to_string()));
}

#[test]
fn should_report_stale_manifest_state() {
    // Arrange
    let (left, _left_dir, projection, left_options) = create_manifest_source("instance-a", "bravo");
    let (right, _right_dir, _, right_options) = create_manifest_source("instance-b", "bravo");
    let left_manifest = left
        .export_projection_verification_manifest(&projection, left_options)
        .expect("left manifest");
    let mut stale_manifest = right
        .export_projection_verification_manifest(&projection, right_options.clone())
        .expect("stale manifest");
    stale_manifest.expires_at_ms = 1;

    // Act
    let stale = left
        .compare_projection_verification_manifests(vec![left_manifest.clone(), stale_manifest])
        .expect("compare stale manifests");

    // Assert
    assert_eq!(stale.state, "stale");
    assert_eq!(stale.stale_manifest_count, 1);
}

#[test]
fn should_reject_incompatible_hash_metadata() {
    // Arrange
    let (left, _left_dir, projection, left_options) = create_manifest_source("instance-a", "bravo");
    let (right, _right_dir, _, right_options) = create_manifest_source("instance-b", "bravo");
    let left_manifest = left
        .export_projection_verification_manifest(&projection, left_options)
        .expect("left manifest");
    let mut incompatible_manifest = right
        .export_projection_verification_manifest(&projection, right_options)
        .expect("incompatible manifest");
    incompatible_manifest.hash.algorithm = "other-hash".to_string();
    incompatible_manifest.manifest_digest = String::new();

    // Act
    let incompatible = left
        .compare_projection_verification_manifests(vec![left_manifest, incompatible_manifest])
        .expect("compare incompatible manifests");

    // Assert
    assert_eq!(incompatible.state, "incompatible");
    assert_eq!(incompatible.incompatible_manifest_count, 1);
    assert!(incompatible
        .diagnostic_sample
        .contains(&"hash-algorithm".to_string()));
}

#[test]
fn should_exclude_sensitive_values_from_manifest() {
    // Arrange
    let (cassie, _dir, projection, mut options) = create_manifest_source("instance-a", "bravo");
    options.include_row_hashes = true;

    // Act
    let manifest = cassie
        .export_projection_verification_manifest(&projection, options)
        .expect("manifest");
    let serialized = serde_json::to_string(&manifest).expect("serialize manifest");

    // Assert
    assert!(!serialized.contains("sensitive body text"));
    assert!(!serialized.contains("secret password bind value"));
    assert!(!serialized.contains("bravo"));
    assert!(!serialized.contains("0.25"));
    assert!(!serialized.contains("0.75"));
}

#[test]
fn should_rehydrate_persisted_consistency_report_after_restart() {
    // Arrange
    with_fallback();
    let path = data_dir("projection_consistency_restart");
    let report_id = {
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE consistency_restart_docs (title TEXT)",
                vec![],
            )
            .expect("create table");
        let projection = canonical_collection("consistency_restart_docs");
        cassie
            .midge
            .put_document(
                &projection,
                Some("doc-1".to_string()),
                serde_json::json!({"title": "alpha"}),
            )
            .expect("insert doc");
        let mut left_options = ProjectionManifestExportOptions::for_instance("instance-a");
        left_options.generated_ms = Some(4_000_000_000_000);
        let mut right_options = ProjectionManifestExportOptions::for_instance("instance-b");
        right_options.generated_ms = Some(4_000_000_000_000);
        let left_manifest = cassie
            .export_projection_verification_manifest(&projection, left_options)
            .expect("left manifest");
        let right_manifest = cassie
            .export_projection_verification_manifest(&projection, right_options)
            .expect("right manifest");

        // Act
        let report = cassie
            .compare_projection_verification_manifests(vec![left_manifest, right_manifest])
            .expect("compare manifests");
        cassie.shutdown();
        report.report_id
    };

    let restarted = Cassie::new_with_data_dir(&path).expect("restarted cassie");
    restarted.startup().expect("restart startup");
    let session = restarted.create_session("tester", None);
    let reports = restarted
        .execute_sql(
            &session,
            &format!(
                "SELECT state, manifest_count FROM pg_catalog.pg_projection_consistency_reports WHERE report_id = '{report_id}'"
            ),
            vec![],
        )
        .expect("query report");

    // Assert
    assert_eq!(
        reports.rows,
        vec![vec![
            Value::String("consistent".to_string()),
            Value::Int64(2)
        ]]
    );

    restarted.shutdown();
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_record_consistency_metrics() {
    // Arrange
    let (left, _left_dir, projection, left_options) = create_manifest_source("instance-a", "bravo");
    let (right, _right_dir, _, right_options) = create_manifest_source("instance-b", "charlie");
    let before = left.metrics();
    let left_manifest = left
        .export_projection_verification_manifest(&projection, left_options)
        .expect("left manifest");
    let right_manifest = right
        .export_projection_verification_manifest(&projection, right_options)
        .expect("right manifest");

    // Act
    let _ = left
        .compare_projection_verification_manifests(vec![left_manifest, right_manifest])
        .expect("compare manifests");
    let after = left.metrics();

    // Assert
    assert_eq!(
        after["projections"]["consistency_exports"].as_u64(),
        before["projections"]["consistency_exports"]
            .as_u64()
            .map(|value| value + 1)
    );
    assert_eq!(
        after["projections"]["consistency_checks"].as_u64(),
        before["projections"]["consistency_checks"]
            .as_u64()
            .map(|value| value + 1)
    );
    assert!(
        after["projections"]["consistency_mismatches"]
            .as_u64()
            .unwrap_or_default()
            >= 1
    );
}

#[test]
fn should_support_admin_rest_manifest_consistency_workflow() {
    // Arrange
    with_fallback();
    let path = data_dir("projection_consistency_rest");
    let config = CassieRuntimeConfig {
        user: "sa".to_string(),
        password: "topsecret".to_string(),
        ..CassieRuntimeConfig::default()
    };
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).expect("cassie");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE consistency_rest_docs (title TEXT)",
                vec![],
            )
            .expect("create table");
        let projection = canonical_collection("consistency_rest_docs");
        cassie
            .midge
            .put_document(
                &projection,
                Some("doc-1".to_string()),
                serde_json::json!({"title": "alpha"}),
            )
            .expect("insert doc");

        let (base_url, server) = spawn_rest_server(cassie.clone()).await;
        let client = reqwest::Client::new();
        let admin_cookie = login_cookie(&client, &base_url).await;
        let nonce = Uuid::new_v4().to_string();

        // Act
        let unauthorized = client
            .post(format!(
                "{base_url}/api/v1/admin/projections/consistency_rest_docs/verification-manifest"
            ))
            .json(&serde_json::json!({"instance_id": format!("unauthorized-{nonce}")}))
            .send()
            .await
            .expect("unauthorized request");
        let manifest = client
            .post(format!(
                "{base_url}/api/v1/admin/projections/consistency_rest_docs/verification-manifest"
            ))
            .header("cookie", &admin_cookie)
            .json(&serde_json::json!({
                "instance_id": "rest-a",
                "generated_ms": 4_000_000_000_000_u64,
                "ttl_ms": 86_400_000_u64,
                "include_row_hashes": true
            }))
            .send()
            .await
            .expect("manifest request")
            .json::<serde_json::Value>()
            .await
            .expect("manifest json");
        let report = client
            .post(format!(
                "{base_url}/api/v1/admin/projection-consistency-checks"
            ))
            .header("cookie", &admin_cookie)
            .json(&serde_json::json!({"manifests": [manifest.clone(), manifest]}))
            .send()
            .await
            .expect("compare request")
            .json::<serde_json::Value>()
            .await
            .expect("report json");

        // Assert
        assert_eq!(unauthorized.status(), reqwest::StatusCode::UNAUTHORIZED);
        assert_eq!(report["state"], "consistent");
        assert_eq!(report["manifest_count"], 2);

        server.abort();
        let _ = server.await;
    });

    cassie.shutdown();
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_support_restful_projection_consistency_aliases() {
    // Arrange
    with_fallback();
    let path = data_dir("projection_consistency_restful_aliases");
    let config = CassieRuntimeConfig {
        user: "sa".to_string(),
        password: "topsecret".to_string(),
        ..CassieRuntimeConfig::default()
    };
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).expect("cassie");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE consistency_rest_alias_docs (title TEXT)",
                vec![],
            )
            .expect("create table");
        let projection = canonical_collection("consistency_rest_alias_docs");
        cassie
            .midge
            .put_document(
                &projection,
                Some("doc-1".to_string()),
                serde_json::json!({"title": "alpha"}),
            )
            .expect("insert doc");

        let (base_url, server) = spawn_rest_server(cassie.clone()).await;
        let client = reqwest::Client::new();
        let admin_cookie = login_cookie(&client, &base_url).await;

        // Act
        let manifest_response = client
            .post(format!(
                "{base_url}/api/v1/admin/projections/consistency_rest_alias_docs/verification-manifests"
            ))
            .header("cookie", &admin_cookie)
            .json(&serde_json::json!({
                "instance_id": "rest-a",
                "generated_ms": 4_000_000_000_000_u64,
                "ttl_ms": 86_400_000_u64,
                "include_row_hashes": true
            }))
            .send()
            .await
            .expect("manifest request");
        let manifest_status = manifest_response.status();
        let manifest = manifest_response
            .json::<serde_json::Value>()
            .await
            .expect("manifest json");

        let report_response = client
            .post(format!("{base_url}/api/v1/admin/projection-consistency-reports"))
            .header("cookie", &admin_cookie)
            .json(&serde_json::json!({"manifests": [manifest.clone(), manifest]}))
            .send()
            .await
            .expect("report request");
        let report_status = report_response.status();
        let report = report_response
            .json::<serde_json::Value>()
            .await
            .expect("report json");

        let report_list_response = client
            .get(format!("{base_url}/api/v1/admin/projection-consistency-reports"))
            .header("cookie", &admin_cookie)
            .send()
            .await
            .expect("reports request");
        let report_list_status = report_list_response.status();
        let reports = report_list_response
            .json::<serde_json::Value>()
            .await
            .expect("reports json");

        // Assert
        assert_eq!(manifest_status, reqwest::StatusCode::OK);
        assert_eq!(report_status, reqwest::StatusCode::OK);
        assert_eq!(report_list_status, reqwest::StatusCode::OK);
        assert_eq!(report["state"], "consistent");
        assert_eq!(report["manifest_count"], 2);
        assert!(
            reports["reports"]
                .as_array()
                .expect("reports")
                .iter()
                .any(|entry| entry["report_id"] == report["report_id"]),
            "expected created report to appear in GET /projection-consistency-reports"
        );

        server.abort();
        let _ = server.await;
    });

    cassie.shutdown();
    let _ = std::fs::remove_dir_all(path);
}
