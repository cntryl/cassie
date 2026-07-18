use std::fs;
use std::path::{Path, PathBuf};

const DELETED_PROGRESS_ARTIFACTS: &[&str] = &[
    "todo.md",
    "docs/read-model-gap-analysis.md",
    "docs/read-model-autopilot-plan.md",
    "docs/performance-rebaseline-phase-10.md",
];

const CANONICAL_DOCS: &[&str] = &[
    "README.md",
    "docs/README.md",
    "docs/feature-support.md",
    "docs/postgres-compatibility.md",
    "docs/performance-contracts.md",
    "docs/production-readiness.md",
    "docs/product-roadmap.md",
];

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(path: impl AsRef<Path>) -> String {
    fs::read_to_string(path).expect("documentation file should be readable")
}

#[test]
fn should_remove_progress_artifacts() {
    // Arrange
    let root = repo_root();
    let canonical = CANONICAL_DOCS
        .iter()
        .map(|path| (*path, read(root.join(path))))
        .collect::<Vec<_>>();

    // Act
    let linked_progress_artifacts = DELETED_PROGRESS_ARTIFACTS
        .iter()
        .filter(|path| {
            root.join(path).exists()
                || canonical
                    .iter()
                    .any(|(_, contents)| contents.contains(path.rsplit('/').next().unwrap_or(path)))
        })
        .copied()
        .collect::<Vec<_>>();

    // Assert
    assert!(
        linked_progress_artifacts.is_empty(),
        "deleted progress artifacts remain present or linked: {linked_progress_artifacts:?}"
    );
}

#[test]
fn should_reject_stale_canonical_claims() {
    // Arrange
    let root = repo_root();
    let canonical = CANONICAL_DOCS
        .iter()
        .map(|path| (*path, read(root.join(path))))
        .collect::<Vec<_>>();

    // Act
    let stale_claims = canonical
        .iter()
        .flat_map(|(path, contents)| {
            contents.lines().filter_map(move |line| {
                let lower = line.to_ascii_lowercase();
                let stale = (0..=99).any(|number| lower.contains(&format!("phase {number}")))
                    || lower.contains("lexkey v")
                    || lower.contains("layout v")
                    || lower.contains("rebaseline")
                    || lower.contains("stable/experimental")
                    || lower.contains("implemented baseline/planned");
                stale.then(|| format!("{path}: {line}"))
            })
        })
        .collect::<Vec<_>>();

    // Assert
    assert!(
        stale_claims.is_empty(),
        "canonical docs contain phase-era, stale-layout, or ambiguous status claims: {stale_claims:#?}"
    );
}

#[test]
fn should_assign_canonical_document_owners() {
    // Arrange
    let docs_index = read(repo_root().join("docs/README.md"));

    // Act
    let ownership_claims = [
        "README.md` owns Cassie's mission",
        "feature-support.md` owns behavior and status",
        "postgres-compatibility.md` owns pgwire and client compatibility",
        "performance-contracts.md` owns performance contracts",
        "production-readiness.md` owns readiness evidence",
    ];

    // Assert
    for claim in ownership_claims {
        assert!(docs_index.contains(claim), "missing owner claim: {claim}");
    }
}

#[test]
fn should_document_trusted_upstream_rest_tls_termination() {
    // Arrange
    let root = repo_root();
    let deployment_contracts = [
        read(root.join("README.md")),
        read(root.join("docs/postgres-compatibility.md")),
        read(root.join("public/openapi.yml")),
    ];
    let compose = read(root.join("compose.yml"));

    // Act
    let compose_enables_upstream_termination =
        compose.contains("CASSIE_ALLOW_INSECURE_NON_LOOPBACK_LISTEN: \"1\"");
    let contracts_document_upstream_termination = deployment_contracts.iter().all(|contents| {
        contents.contains("CASSIE_ALLOW_INSECURE_NON_LOOPBACK_LISTEN")
            && contents.contains("load balancer")
    });

    // Assert
    assert!(compose_enables_upstream_termination);
    assert!(
        contracts_document_upstream_termination,
        "REST transport docs must distinguish direct TLS from trusted load-balancer termination"
    );
}

#[test]
fn should_define_storage_ownership_boundary() {
    // Arrange
    let feature_support = read(repo_root().join("docs/feature-support.md"));

    // Act
    let owns_midge_mechanics =
        feature_support.contains("Midge owns persistence, durability, and recovery mechanics");
    let owns_query_behavior =
        feature_support.contains("Cassie owns logical query layouts and query-visible failures");

    // Assert
    assert!(feature_support.contains("cassie-midge-layout-v1"));
    assert!(owns_midge_mechanics);
    assert!(owns_query_behavior);
}
