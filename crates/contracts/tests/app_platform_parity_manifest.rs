use a3s_cloud_contracts::{
    AppPlatformCapabilityAvailability, AppPlatformCapabilityCategory, AppPlatformGateState,
    AppPlatformParityManifest,
};
use std::collections::BTreeMap;
use std::path::Path;

const MANIFEST: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/app-platform/v1/parity-manifest.acl"
));

#[test]
fn checked_in_manifest_is_canonical_complete_and_not_publicly_advertised() {
    let manifest = AppPlatformParityManifest::parse_acl(MANIFEST).expect("manifest");
    assert_eq!(manifest.baseline(), "2026-08-13");
    assert_eq!(manifest.public_claim_gate(), "APP0.6");
    assert_eq!(
        manifest
            .gates()
            .iter()
            .find(|gate| gate.id() == "APP0.1")
            .expect("APP0.1 gate")
            .state(),
        AppPlatformGateState::Verified
    );
    assert_eq!(
        manifest
            .gates()
            .iter()
            .find(|gate| gate.id() == "APP0.2")
            .expect("APP0.2 gate")
            .state(),
        AppPlatformGateState::InProgress
    );
    assert!(manifest.digest().starts_with("sha256:"));
    assert_eq!(manifest.canonical_acl(), MANIFEST.replace("\r\n", "\n"));
    assert!(!manifest.parity_claim());
    assert_eq!(manifest.references().len(), 8);
    assert!(manifest.references().iter().all(|reference| {
        reference.observed_on() == manifest.baseline() && reference.url().starts_with("https://")
    }));
    assert_eq!(
        AppPlatformParityManifest::restore(MANIFEST, manifest.digest()).expect("restored"),
        manifest
    );
    assert!(
        AppPlatformParityManifest::restore(MANIFEST, &format!("sha256:{}", "f".repeat(64)))
            .is_err()
    );

    let counts = manifest.capabilities().iter().fold(
        BTreeMap::<AppPlatformCapabilityCategory, usize>::new(),
        |mut counts, capability| {
            *counts.entry(capability.category()).or_default() += 1;
            counts
        },
    );
    assert_eq!(counts[&AppPlatformCapabilityCategory::ApplicationMode], 6);
    assert_eq!(counts[&AppPlatformCapabilityCategory::AuthoringToolkit], 22);
    assert_eq!(counts[&AppPlatformCapabilityCategory::Node], 23);
    assert_eq!(counts[&AppPlatformCapabilityCategory::Plugin], 6);
    assert_eq!(counts[&AppPlatformCapabilityCategory::Knowledge], 13);
    assert_eq!(
        counts[&AppPlatformCapabilityCategory::PublicationChannel],
        6
    );
    assert_eq!(counts[&AppPlatformCapabilityCategory::Monitoring], 7);
    assert_eq!(counts[&AppPlatformCapabilityCategory::Enterprise], 8);
    assert_eq!(manifest.capabilities().len(), 91);
    assert!(manifest
        .capabilities()
        .iter()
        .all(|capability| !capability.references().is_empty()));
    assert!(manifest.capabilities().iter().all(|capability| {
        capability.availability() != AppPlatformCapabilityAvailability::Public
    }));
    assert_eq!(
        manifest
            .capabilities()
            .iter()
            .filter(|capability| {
                capability.availability() == AppPlatformCapabilityAvailability::Internal
            })
            .map(|capability| capability.id())
            .collect::<Vec<_>>(),
        [
            "enterprise.organizations-workspaces",
            "node.human-input",
            "node.if-else",
            "node.output",
            "node.template",
            "node.user-input",
        ]
    );
}

#[test]
fn node_owners_follow_the_accepted_execution_boundaries() {
    let manifest = AppPlatformParityManifest::parse_acl(MANIFEST).expect("manifest");
    let owners = manifest
        .capabilities()
        .iter()
        .filter(|capability| capability.category() == AppPlatformCapabilityCategory::Node)
        .map(|capability| (capability.id(), capability.owner()))
        .collect::<BTreeMap<_, _>>();

    assert_eq!(owners["node.code"], "executions");
    assert_eq!(owners["node.http-request"], "connectors");
    assert_eq!(owners["node.schedule-trigger"], "automations");
    assert_eq!(owners["node.answer"], "applications");
    assert_eq!(owners["node.output"], "workflow");
}

#[test]
fn checked_in_manifest_evidence_references_repository_files() {
    let manifest = AppPlatformParityManifest::parse_acl(MANIFEST).expect("manifest");
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");

    for evidence in manifest
        .gates()
        .iter()
        .flat_map(|gate| gate.evidence())
        .chain(
            manifest
                .capabilities()
                .iter()
                .flat_map(|capability| capability.evidence()),
        )
    {
        let (_, reference) = evidence.split_once(':').expect("typed evidence");
        let path = reference.split('#').next().expect("evidence path");
        assert!(
            repository.join(path).is_file(),
            "evidence path does not exist: {path}"
        );
    }
}

#[test]
fn parser_rejects_schema_drift_incomplete_inventory_and_false_public_claims() {
    let manifest = MANIFEST.replace("\r\n", "\n");
    let unknown = manifest.replacen(
        "  baseline = \"2026-08-13\"",
        "  baseline = \"2026-08-13\"\n  legacy_mode = true",
        1,
    );
    assert!(AppPlatformParityManifest::parse_acl(&unknown).is_err());

    let incomplete = manifest.replacen(
        "capability \"node.output\"",
        "capability \"node.output-renamed\"",
        1,
    );
    assert_ne!(incomplete, manifest);
    assert!(AppPlatformParityManifest::parse_acl(&incomplete).is_err());

    let duplicate = manifest.replacen(
        "capability \"node.output\"",
        "capability \"node.answer\"",
        1,
    );
    assert_ne!(duplicate, manifest);
    assert!(AppPlatformParityManifest::parse_acl(&duplicate).is_err());

    let false_public_claim = manifest.replacen(
        "capability \"node.output\" {\n    availability = \"internal\"",
        "capability \"node.output\" {\n    availability = \"public\"",
        1,
    );
    assert_ne!(false_public_claim, manifest);
    assert!(AppPlatformParityManifest::parse_acl(&false_public_claim).is_err());

    let untyped_evidence = manifest.replacen("doc:ROADMAP.md", "url:https://example.invalid", 1);
    assert_ne!(untyped_evidence, manifest);
    assert!(AppPlatformParityManifest::parse_acl(&untyped_evidence).is_err());

    let unknown_reference = manifest.replacen(
        "references = [\"workflow-chatflow\"]",
        "references = [\"unknown-reference\"]",
        1,
    );
    assert_ne!(unknown_reference, manifest);
    assert!(AppPlatformParityManifest::parse_acl(&unknown_reference).is_err());

    let changed_source = manifest.replacen(
        "https://docs.dify.ai/llms.txt",
        "https://docs.dify.ai/other",
        1,
    );
    assert_ne!(changed_source, manifest);
    assert!(AppPlatformParityManifest::parse_acl(&changed_source).is_err());
}

#[test]
fn parser_rejects_noncanonical_acl_bytes() {
    assert!(AppPlatformParityManifest::parse_acl(&format!("\n{MANIFEST}")).is_err());
}

#[test]
fn every_advertised_public_capability_requires_verified_gates_and_test_evidence() {
    let source = r#"parity_manifest "dify-commercial-core-2026-08-13" {
  baseline = "2026-08-13"
  parity_claim = false
  public_claim_gate = "APP0.6"
  schema = "a3s.cloud.app-platform.parity-manifest.v1"
  reference "workflow-chatflow" {
    observed_on = "2026-08-13"
    url = "https://docs.dify.ai/en/cloud/use-dify/build/workflow-chatflow"
  }
  gate "APP0.6" {
    evidence = ["doc:ROADMAP.md"]
    state = "planned"
  }
  gate "W0.3" {
    evidence = ["test:crates/contracts/tests/app_platform_parity_manifest.rs"]
    state = "in_progress"
  }
  capability "node.output" {
    availability = "public"
    category = "node"
    dependencies = []
    evidence = ["implementation:crates/contracts/src/lib.rs"]
    gate = "W0.3"
    label = "Output"
    owner = "workflow"
    references = ["workflow-chatflow"]
  }
}
"#;
    let error = AppPlatformParityManifest::parse_acl(source).expect_err("unverified public gate");
    assert!(error.contains("unverified owning gate"), "{error}");

    let verified_owner = source.replace("state = \"in_progress\"", "state = \"verified\"");
    let unverified_dependency =
        verified_owner.replace("dependencies = []", "dependencies = [\"APP0.6\"]");
    let error = AppPlatformParityManifest::parse_acl(&unverified_dependency)
        .expect_err("unverified dependency");
    assert!(error.contains("unverified dependency"), "{error}");

    let error = AppPlatformParityManifest::parse_acl(&verified_owner)
        .expect_err("public capability without test evidence");
    assert!(error.contains("requires test evidence"), "{error}");
}

#[test]
fn authority_decision_register_is_complete_and_manifest_references_it() {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let register =
        std::fs::read_to_string(repository.join("docs/decisions/app-platform/README.md"))
            .expect("decision register");
    let decisions = register
        .lines()
        .filter_map(|line| {
            line.split_once("](")?
                .1
                .split_once(')')
                .map(|value| value.0)
        })
        .filter(|target| target.ends_with(".md"))
        .collect::<Vec<_>>();
    assert_eq!(
        decisions.len(),
        33,
        "decision register changed unexpectedly"
    );
    for decision in decisions {
        let body = std::fs::read_to_string(
            repository
                .join("docs/decisions/app-platform")
                .join(decision),
        )
        .expect("decision body");
        assert!(
            body.contains("Status: Accepted"),
            "{decision} is not accepted"
        );
        assert!(body.contains("## Decision"), "{decision} has no decision");
        assert!(
            body.contains("## Consequences"),
            "{decision} has no consequences"
        );
    }

    assert!(MANIFEST.contains("doc:docs/decisions/app-platform/0002-application-delivery.md"));
}
