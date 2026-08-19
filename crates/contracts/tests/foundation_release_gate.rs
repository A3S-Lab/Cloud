use std::fs;
use std::path::{Path, PathBuf};

const FOUNDATION_PROVIDER_TEST: &str = "postgres_foundation_is_migrated_atomic_and_idempotent";

#[test]
fn f0_provider_recertification_is_mandatory_in_ci() {
    let workflow = read_repository_file(".github/workflows/ci.yml");
    assert_eq!(
        workflow.matches(FOUNDATION_PROVIDER_TEST).count(),
        1,
        "CI must invoke the complete F0 provider test exactly once"
    );

    let nats_start = workflow
        .find("- name: Start checksum-pinned NATS notification fixture")
        .expect("CI must start the real NATS fixture");
    let foundation_gate = workflow
        .find("- name: Certify the F0 PostgreSQL and Event foundation")
        .expect("CI must expose the F0 provider gate as a named release boundary");
    let nats_stop = workflow
        .find("- name: Stop checksum-pinned NATS notification fixture")
        .expect("CI must stop the real NATS fixture");
    assert!(
        nats_start < foundation_gate && foundation_gate < nats_stop,
        "the F0 gate must run while the checksum-pinned NATS fixture is alive"
    );

    let gate = &workflow[foundation_gate..nats_stop];
    for required in [
        "A3S_CLOUD_TEST_NATS_URL: nats://127.0.0.1:4222",
        "A3S_CLOUD_TEST_OFFLINE_SOURCE_RESOLVER: \"1\"",
        "cargo test --locked -p a3s-cloud-control-plane",
        "--test postgres_integration",
        FOUNDATION_PROVIDER_TEST,
        "-- --exact --nocapture --test-threads=1",
    ] {
        assert!(
            gate.contains(required),
            "the F0 provider gate lost required evidence configuration {required:?}"
        );
    }
    assert!(
        workflow[..foundation_gate].contains("A3S_CLOUD_TEST_POSTGRES_URL:"),
        "the F0 provider gate must inherit a real PostgreSQL fixture URL"
    );
}

#[test]
fn f0_status_uses_the_locked_foundation_versions() {
    let roadmap = read_repository_file("ROADMAP.md");
    let roadmap_row = roadmap
        .lines()
        .find(|line| line.starts_with("| `F0` \u{2014} Foundation |"))
        .expect("ROADMAP must contain the authoritative F0 status row");
    assert_locked_versions("ROADMAP F0 status", roadmap_row);

    let plan = read_repository_file("docs/development-plan.md");
    let plan_row = plan
        .lines()
        .find(|line| line.starts_with("| F0 |"))
        .expect("development plan must contain the detailed F0 status row");
    assert_locked_versions("development-plan F0 status", plan_row);
}

fn assert_locked_versions(owner: &str, claim: &str) {
    for required in ["Flow `1.0.0`", "Boot `0.2.0`", "ORM `0.3.1`"] {
        assert!(
            claim.contains(required),
            "{owner} drifted from the locked foundation dependency {required}"
        );
    }
}

fn read_repository_file(relative: &str) -> String {
    fs::read_to_string(repository_root().join(relative))
        .unwrap_or_else(|error| panic!("read repository file {relative}: {error}"))
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}
