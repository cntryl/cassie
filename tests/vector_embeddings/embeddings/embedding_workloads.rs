//! #1365: network-free checks of the embedding workload machinery. The corpus and the
//! generated vectors are never needed; every fixture here is hand-checkable.

#[path = "../../../benches/support/embedding_workloads.rs"]
mod workloads;

use workloads::ground_truth::{
    decode_truths, distance, encode_truths, exact_top_k, recall_at_k, Metric, Truth,
};
use workloads::manifest::{embeddings_dir, in_tenant, tenant_bucket, Manifest, README_HINT};
use workloads::statistics::{mean, percentile};
use workloads::vector_file::{
    decode_f32_rows, hex, read_f32_rows, read_lines, sha256_file, verify_checksum,
};

use crate::support_temp_dirs::IsolatedDataDir;

const MANIFEST_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/corpus/embeddings/manifest.json"
);

fn encode_f32_rows(values: &[f32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

fn fixture_dir(label: &str) -> IsolatedDataDir {
    let dir = IsolatedDataDir::new(label);
    std::fs::create_dir_all(dir.path()).expect("create fixture directory");
    dir
}

fn manifest_text() -> String {
    std::fs::read_to_string(MANIFEST_PATH).expect("committed manifest")
}

fn manifest_value() -> serde_json::Value {
    serde_json::from_str(&manifest_text()).expect("manifest json")
}

#[test]
fn should_validate_the_committed_manifest() {
    // Arrange
    let path = std::path::Path::new(MANIFEST_PATH);

    // Act
    let manifest = Manifest::load(path).expect("manifest should load");

    // Assert
    manifest.validate().expect("manifest should be consistent");
    assert_eq!(manifest.ground_truth.k, 20);
    assert!(manifest
        .ground_truth
        .metrics
        .contains(&"cosine".to_string()));
    assert!(manifest.ground_truth.metrics.contains(&"l2".to_string()));
    assert_eq!(manifest.set("10k").expect("10k set").documents, 10_000);
    assert_eq!(manifest.set("100k").expect("100k set").documents, 100_000);
    assert!(manifest.set("1m").is_none());
}

#[test]
fn should_pin_two_models_with_licenses_revisions() {
    // Arrange
    let manifest = Manifest::load(std::path::Path::new(MANIFEST_PATH)).expect("manifest");
    let raw = manifest_value();

    // Act
    let small = manifest.model("bge-small").expect("small model");
    let base = manifest.model("bge-base").expect("base model");

    // Assert
    assert_eq!(small.dimensions, 384);
    assert!(base.dimensions >= 768);
    for model in [small, base] {
        assert_eq!(model.revision.len(), 40);
        assert!(model.revision.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert!(model.id.starts_with("BAAI/bge-"));
        assert!(model.file_stem.contains("bge"));
    }
    for entry in raw["models"].as_array().expect("models") {
        assert!(entry["license"]
            .as_str()
            .expect("license")
            .starts_with("MIT"));
    }
    assert_eq!(
        raw["corpus"]["source"]["beir_dataset_revision"]
            .as_str()
            .map(str::len),
        Some(40)
    );
    assert!(raw["redistribution"]
        .as_str()
        .expect("statement")
        .contains("No corpus text"));
}

#[test]
fn should_keep_tuning_held_out_queries_disjoint_with_declared_counts() {
    // Arrange
    let manifest = Manifest::load(std::path::Path::new(MANIFEST_PATH)).expect("manifest");

    // Act
    let tuning = &manifest.queries.tuning;
    let held_out = &manifest.queries.held_out;

    // Assert
    assert_eq!(tuning.ids.len(), tuning.count);
    assert_eq!(held_out.ids.len(), held_out.count);
    assert!(tuning.ids.iter().all(|id| !held_out.ids.contains(id)));
}

#[test]
fn should_reject_a_manifest_whose_query_splits_overlap() {
    // Arrange
    let mut value = manifest_value();
    value["queries"]["tuning"]["ids"] = serde_json::json!(["android/1", "android/2"]);
    value["queries"]["tuning"]["count"] = serde_json::json!(2);
    value["queries"]["held_out"]["ids"] = serde_json::json!(["android/2", "android/3"]);
    value["queries"]["held_out"]["count"] = serde_json::json!(2);
    let manifest = Manifest::parse(&value.to_string()).expect("parse");

    // Act
    let error = manifest.validate().expect_err("overlap must be rejected");

    // Assert
    assert!(error.contains("android/2"), "{error}");
}

#[test]
fn should_reject_a_file_entry_whose_byte_count_disagrees_with_its_shape() {
    // Arrange
    let mut value = manifest_value();
    value["files"] = serde_json::json!([{
        "name": "cqadupstack-bge-small-en-v1.5-10k.f32",
        "sha256": "0".repeat(64),
        "bytes": 12,
        "rows": 10_000,
        "dimensions": 384
    }]);
    let manifest = Manifest::parse(&value.to_string()).expect("parse");

    // Act
    let error = manifest
        .validate()
        .expect_err("size mismatch must be rejected");

    // Assert
    assert!(error.contains("rows x dimensions x 4"), "{error}");
}

#[test]
fn should_reject_a_recorded_file_with_the_wrong_row_count_for_its_set() {
    // Arrange
    let mut value = manifest_value();
    value["files"] = serde_json::json!([{
        "name": "cqadupstack-bge-small-en-v1.5-10k.f32",
        "sha256": "1".repeat(64),
        "bytes": 9_999 * 384 * 4,
        "rows": 9_999,
        "dimensions": 384
    }]);
    let manifest = Manifest::parse(&value.to_string()).expect("parse");

    // Act
    let error = manifest
        .validate()
        .expect_err("row count must match the set");

    // Assert
    assert!(error.contains("expected rows=10000"), "{error}");
}

#[test]
fn should_name_the_four_files_of_one_model_set_pair() {
    // Arrange
    let manifest = Manifest::load(std::path::Path::new(MANIFEST_PATH)).expect("manifest");
    let model = manifest.model("bge-base").expect("model");
    let set = manifest.set("10k").expect("set");

    // Act
    let names = Manifest::file_names(model, set);

    // Assert
    assert_eq!(names[0], "cqadupstack-bge-base-en-v1.5-10k.f32");
    assert_eq!(names[1], "cqadupstack-10k.ids.txt");
    assert_eq!(names[2], "cqadupstack-bge-base-en-v1.5-queries.f32");
    assert_eq!(names[3], "cqadupstack-queries.ids.txt");
    assert!(manifest.file("cqadupstack-not-recorded.f32").is_none());
    assert_eq!(
        embeddings_dir(std::path::Path::new("/repo")),
        std::path::Path::new("/repo/target/embeddings")
    );
}

#[test]
fn should_assign_tenants_by_document_id_hash_with_nested_selectivities() {
    // Arrange
    let ids: Vec<String> = (0..20_000).map(|n| format!("android/{n}")).collect();

    // Act
    let counts: Vec<usize> = [1, 10, 50]
        .into_iter()
        .map(|percent| ids.iter().filter(|id| in_tenant(id, percent)).count())
        .collect();

    // Assert
    assert_eq!(tenant_bucket("android/1"), 80);
    assert_eq!(tenant_bucket("gaming/42"), 43);
    assert!(ids.iter().all(|id| !in_tenant(id, 1) || in_tenant(id, 10)));
    assert!(ids.iter().all(|id| !in_tenant(id, 10) || in_tenant(id, 50)));
    assert!((120..=280).contains(&counts[0]), "1% was {}", counts[0]);
    assert!(
        (1_700..=2_300).contains(&counts[1]),
        "10% was {}",
        counts[1]
    );
    assert!(
        (9_500..=10_500).contains(&counts[2]),
        "50% was {}",
        counts[2]
    );
}

#[test]
fn should_round_trip_f32_rows_through_a_verified_checksum() {
    // Arrange
    let dir = fixture_dir("embedding-workloads-rows");
    let path = dir.path().join("tiny.f32");
    let values = [1.0_f32, -2.5, 0.25, 8.0, 3.0, -0.0];
    std::fs::write(&path, encode_f32_rows(&values)).expect("write rows");
    let expected = sha256_file(&path).expect("sha");

    // Act
    let rows = read_f32_rows(&path, 3).expect("two rows of three");
    let verified = verify_checksum(&path, &expected, README_HINT);

    // Assert
    assert_eq!(rows, values);
    assert!(verified.is_ok());
    assert_eq!(std::fs::metadata(&path).expect("meta").len(), 24);
    assert_eq!(hex(&[0x00, 0xab, 0xff]), "00abff");
}

#[test]
fn should_match_a_known_sha256_of_a_known_file() {
    // Arrange
    let dir = fixture_dir("embedding-workloads-sha");
    let path = dir.path().join("abc.txt");
    std::fs::write(&path, "abc").expect("write");

    // Act
    let digest = sha256_file(&path).expect("sha");

    // Assert
    assert_eq!(
        digest,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn should_fail_with_a_readme_pointer_when_a_file_is_missing_or_changed() {
    // Arrange
    let dir = fixture_dir("embedding-workloads-verify");
    let missing = dir.path().join("absent.f32");
    let changed = dir.path().join("changed.f32");
    std::fs::write(&changed, encode_f32_rows(&[1.0, 2.0])).expect("write");

    // Act
    let missing_error =
        verify_checksum(&missing, &"0".repeat(64), README_HINT).expect_err("missing");
    let changed_error =
        verify_checksum(&changed, &"0".repeat(64), README_HINT).expect_err("changed");

    // Assert
    assert!(missing_error.contains("is missing"), "{missing_error}");
    assert!(
        missing_error.contains("tools/embeddings/README.md"),
        "{missing_error}"
    );
    assert!(
        changed_error.contains("manifest records"),
        "{changed_error}"
    );
    assert!(
        changed_error.contains("tools/embeddings/README.md"),
        "{changed_error}"
    );
}

#[test]
fn should_reject_bytes_that_are_not_whole_rows() {
    // Arrange
    let dir = fixture_dir("embedding-workloads-ids");
    let ids = dir.path().join("ids.txt");
    std::fs::write(&ids, "android/1\ngis/2\n").expect("write ids");

    // Act
    let partial = decode_f32_rows(&[0_u8; 12], 2);
    let lines = read_lines(&ids).expect("lines");

    // Assert
    assert!(partial.is_err());
    assert!(decode_f32_rows(&[], 0).is_err());
    assert_eq!(lines, ["android/1", "gis/2"]);
}

#[test]
fn should_break_exact_ties_by_row_ordinal_with_boundary_ties_correct() {
    // Arrange: query at the origin; distances 1, 1, 2, 0.5, 1 for rows 0..5.
    let rows = [1.0_f32, 0.0, 0.0, 1.0, 2.0, 0.0, 0.0, 0.5, 1.0, 0.0];
    let query = [0.0_f32, 0.0];

    // Act
    let truth = exact_top_k(Metric::L2, &rows, 2, &query, None, 3);
    let distance_of = |row: usize| distance(Metric::L2, &rows[row * 2..row * 2 + 2], &query);

    // Assert: 0.5 (row 3), then the 1.0 ties in ordinal order (rows 0, 1); row 4 ties the boundary.
    assert_eq!(truth.rows, [3, 0, 1]);
    assert!((truth.boundary - 1.0).abs() < f64::EPSILON);
    assert!((recall_at_k(&truth, &[3, 0, 4], distance_of) - 1.0).abs() < f64::EPSILON);
    assert!((recall_at_k(&truth, &[3, 0, 2], distance_of) - 2.0 / 3.0).abs() < 1e-12);
}

#[test]
fn should_rank_only_the_tenant_subset_when_one_is_given() {
    // Arrange
    let rows = [0.0_f32, 0.0, 5.0, 0.0, 1.0, 0.0, 3.0, 0.0];
    let query = [0.0_f32, 0.0];

    // Act
    let truth = exact_top_k(Metric::L2, &rows, 2, &query, Some(&[1, 3]), 5);

    // Assert
    assert_eq!(truth.rows, [3, 1]);
    assert!((truth.boundary - 5.0).abs() < f64::EPSILON);
}

#[test]
fn should_compute_cosine_distance_with_a_zero_operand_defined_as_one() {
    // Arrange
    let query = [1.0_f32, 0.0];

    // Act
    let same = distance(Metric::Cosine, &[2.0, 0.0], &query);
    let orthogonal = distance(Metric::Cosine, &[0.0, 3.0], &query);
    let opposite = distance(Metric::Cosine, &[-1.0, 0.0], &query);
    let zero = distance(Metric::Cosine, &[0.0, 0.0], &query);

    // Assert
    assert!(same.abs() < 1e-12);
    assert!((orthogonal - 1.0).abs() < 1e-12);
    assert!((opposite - 2.0).abs() < 1e-12);
    assert!((zero - 1.0).abs() < f64::EPSILON);
    assert_eq!(Metric::Cosine.operator(), "<=>");
    assert_eq!(Metric::L2.operator(), "<->");
    assert_eq!(Metric::L2.name(), "l2");
}

#[test]
fn should_compute_recall_at_k_from_a_hand_counted_example() {
    // Arrange: truth {1, 2, 3, 4}; boundary distance is 4.0 and no other row is tied.
    let truth = Truth {
        rows: vec![1, 2, 3, 4],
        boundary: 4.0,
    };
    let distance_of = |row: usize| f64::from(u32::try_from(row).expect("small row"));

    // Act
    let half = recall_at_k(&truth, &[1, 2, 9, 8], distance_of);
    let duplicate = recall_at_k(&truth, &[1, 1, 1, 1], distance_of);
    let perfect = recall_at_k(&truth, &[4, 3, 2, 1], distance_of);
    let empty = recall_at_k(
        &Truth {
            rows: Vec::new(),
            boundary: f64::INFINITY,
        },
        &[],
        distance_of,
    );

    // Assert
    assert!((half - 0.5).abs() < f64::EPSILON);
    assert!((duplicate - 0.25).abs() < f64::EPSILON);
    assert!((perfect - 1.0).abs() < f64::EPSILON);
    assert!((empty - 1.0).abs() < f64::EPSILON);
}

#[test]
fn should_round_trip_ground_truth_through_its_cache_text() {
    // Arrange
    let truths = vec![
        Truth {
            rows: vec![3, 0, 1],
            boundary: 0.123_456_789_012_345_67,
        },
        Truth {
            rows: Vec::new(),
            boundary: f64::INFINITY,
        },
    ];

    // Act
    let decoded = decode_truths(&encode_truths(&truths)).expect("decode");

    // Assert
    assert_eq!(decoded, truths);
    assert!(decode_truths("not-a-number 1 2").is_err());
}

#[test]
fn should_report_nearest_rank_percentiles_with_means() {
    // Arrange
    let values: Vec<f64> = (1..=20).map(f64::from).collect();

    // Act
    let (p50, p95, p100) = (
        percentile(&values, 50),
        percentile(&values, 95),
        percentile(&values, 100),
    );

    // Assert
    assert!((p50 - 10.0).abs() < f64::EPSILON);
    assert!((p95 - 19.0).abs() < f64::EPSILON);
    assert!((p100 - 20.0).abs() < f64::EPSILON);
    assert!((mean(&values) - 10.5).abs() < f64::EPSILON);
    assert!(percentile(&[], 95).abs() < f64::EPSILON);
    assert!(mean(&[]).abs() < f64::EPSILON);
}

#[test]
fn should_generate_the_synthetic_control_bit_identically_on_every_call() {
    // Arrange
    let manifest = Manifest::load(std::path::Path::new(MANIFEST_PATH)).expect("manifest");
    let control = &manifest.synthetic_control;

    // Act
    let first = workloads::synthetic::generate(control, 50, 7);
    let second = workloads::synthetic::generate(control, 50, 7);

    // Assert
    assert_eq!(first.documents, second.documents);
    assert_eq!(first.queries, second.queries);
    assert_eq!(first.documents.len(), 50 * control.dimensions);
}

#[test]
fn should_generate_unit_length_synthetic_vectors() {
    // Arrange
    let manifest = Manifest::load(std::path::Path::new(MANIFEST_PATH)).expect("manifest");
    let control = &manifest.synthetic_control;

    // Act
    let vectors = workloads::synthetic::generate(control, 20, 0);

    // Assert
    for row in vectors.documents.chunks(control.dimensions) {
        let norm: f32 = row.iter().map(|value| value * value).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-4, "row norm {norm}");
    }
}

#[test]
fn should_generate_synthetic_queries_from_a_stream_other_than_the_documents() {
    // Arrange
    let manifest = Manifest::load(std::path::Path::new(MANIFEST_PATH)).expect("manifest");
    let control = &manifest.synthetic_control;

    // Act
    let vectors = workloads::synthetic::generate(control, 5, 5);

    // Assert
    assert_ne!(vectors.documents, vectors.queries);
}

#[test]
fn should_match_the_recorded_synthetic_checksums_of_the_10k_set() {
    // Arrange
    let manifest = Manifest::load(std::path::Path::new(MANIFEST_PATH)).expect("manifest");
    let control = &manifest.synthetic_control;
    let queries = manifest.queries.tuning.count + manifest.queries.held_out.count;
    let documents = manifest.set("10k").expect("10k set").documents;

    // Act
    let vectors = workloads::synthetic::generate(control, documents, queries);

    // Assert
    assert_eq!(
        control.sha256.get("10k"),
        Some(&workloads::synthetic::checksum(&vectors.documents))
    );
    assert_eq!(
        control.sha256.get("queries"),
        Some(&workloads::synthetic::checksum(&vectors.queries))
    );
}

#[test]
fn should_reject_a_tuning_point_outside_the_swept_candidates() {
    // Arrange
    let mut value = manifest_value();
    value["index_tuning"]["hnsw"]["ef_search_points"] = serde_json::json!([1280, 77]);
    let manifest = Manifest::parse(&value.to_string()).expect("parse");

    // Act
    let error = manifest
        .validate()
        .expect_err("an unswept point is rejected");

    // Assert
    assert!(error.contains("77"), "{error}");
}

#[test]
fn should_require_at_least_two_tuning_points_per_index() {
    // Arrange
    let mut value = manifest_value();
    value["index_tuning"]["ivfflat"]["probe_permille_points"] = serde_json::json!([80]);
    let manifest = Manifest::parse(&value.to_string()).expect("parse");

    // Act
    let error = manifest.validate().expect_err("one point is rejected");

    // Assert
    assert!(error.contains("two tuning points"), "{error}");
}
