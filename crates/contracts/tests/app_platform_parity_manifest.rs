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
        AppPlatformGateState::Implemented
    );
    assert_eq!(
        manifest
            .gates()
            .iter()
            .find(|gate| gate.id() == "APP0.3")
            .expect("APP0.3 gate")
            .state(),
        AppPlatformGateState::Implemented
    );
    assert_eq!(
        manifest
            .gates()
            .iter()
            .find(|gate| gate.id() == "APP0.4")
            .expect("APP0.4 gate")
            .state(),
        AppPlatformGateState::Implemented
    );
    assert_eq!(
        manifest
            .gates()
            .iter()
            .find(|gate| gate.id() == "APP0.5")
            .expect("APP0.5 gate")
            .state(),
        AppPlatformGateState::Implemented
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
            "application.chatbot",
            "application.chatflow",
            "application.classic-agent",
            "application.new-agent",
            "application.text-generator",
            "application.workflow",
            "enterprise.audit-security",
            "enterprise.custom-domain-branding",
            "enterprise.external-identity",
            "enterprise.ha-disaster-recovery",
            "enterprise.isolation-quota-retention",
            "enterprise.organizations-workspaces",
            "enterprise.saml-oidc-scim",
            "monitoring.feedback-review",
            "monitoring.usage-cost",
            "node.answer",
            "node.http-request",
            "node.human-input",
            "node.if-else",
            "node.iteration",
            "node.list-operator",
            "node.loop",
            "node.output",
            "node.schedule-trigger",
            "node.template",
            "node.user-input",
            "node.variable-aggregator",
            "node.webhook-trigger",
            "publication.api-blocking",
            "publication.api-streaming",
            "toolkit.annotation-reply",
            "toolkit.citations",
            "toolkit.file-input",
            "toolkit.more-like-this",
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
        "https://github.com/A3S-Lab/Cloud/blob/main/docs/ai-application-platform-plan.md",
        "https://github.com/A3S-Lab/Cloud/blob/main/docs/domain-model.md",
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
    let source = r#"parity_manifest "application-platform-core-2026-08-13" {
  baseline = "2026-08-13"
  parity_claim = false
  public_claim_gate = "APP0.6"
  schema = "a3s.cloud.app-platform.parity-manifest.v1"
  reference "workflow-chatflow" {
    observed_on = "2026-08-13"
    url = "https://github.com/A3S-Lab/Cloud/blob/main/docs/decisions/app-platform/0009-workflow-node-catalog-projection.md"
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
fn authority_decision_register_matches_files_and_latest_decision_is_manifested() {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let decision_directory = repository.join("docs/decisions/app-platform");
    let register =
        std::fs::read_to_string(decision_directory.join("README.md")).expect("decision register");
    let decisions = register
        .lines()
        .filter(|line| line.starts_with("| ["))
        .filter_map(|line| {
            line.split_once("](")?
                .1
                .split_once(')')
                .map(|value| value.0)
        })
        .filter(|target| target.ends_with(".md"))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let mut decision_files = std::fs::read_dir(&decision_directory)
        .expect("decision directory")
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name != "README.md" && name.ends_with(".md"))
        .collect::<Vec<_>>();
    decision_files.sort();
    assert_eq!(
        decisions, decision_files,
        "decision register must exactly match the accepted decision files"
    );

    for decision in &decisions {
        let body =
            std::fs::read_to_string(decision_directory.join(decision)).expect("decision body");
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

    let latest_decision = decisions.last().expect("at least one accepted decision");
    let latest_evidence = format!("doc:docs/decisions/app-platform/{latest_decision}");
    assert!(
        MANIFEST.contains(&latest_evidence),
        "latest accepted decision {latest_decision} must be parity-manifest evidence"
    );

    assert!(MANIFEST.contains("doc:docs/decisions/app-platform/0002-application-delivery.md"));
    assert!(MANIFEST.contains(
        "doc:docs/decisions/app-platform/0045-descriptor-bound-application-variable-failure-routes.md"
    ));
    assert!(MANIFEST.contains(
        "doc:docs/decisions/app-platform/0046-descriptor-bound-application-answer-failure-routes.md"
    ));
    assert!(MANIFEST.contains(
        "implementation:migrations/143_workflow_application_answer_step_projections.sql"
    ));
    assert!(MANIFEST.contains(
        "doc:docs/decisions/app-platform/0047-descriptor-bound-transform-failure-routes.md"
    ));
    assert!(MANIFEST.contains(
        "doc:docs/decisions/app-platform/0048-descriptor-bound-output-failure-routes.md"
    ));
    assert!(MANIFEST
        .contains("implementation:migrations/145_workflow_transform_failure_step_projections.sql"));
    assert!(MANIFEST
        .contains("doc:docs/decisions/app-platform/0051-workflow-local-variable-aggregation.md"));
    assert!(MANIFEST.contains("contract:contracts/w0.3/variable-aggregate.acl"));
    assert!(
        MANIFEST.contains("doc:docs/decisions/app-platform/0052-workflow-local-list-operations.md")
    );
}

#[test]
fn app04_production_foundation_claims_six_modes_and_defers_toolkits_channels() {
    let manifest = AppPlatformParityManifest::parse_acl(MANIFEST).expect("manifest");
    assert_eq!(
        manifest
            .gates()
            .iter()
            .find(|gate| gate.id() == "APP0.4")
            .expect("APP0.4 gate")
            .state(),
        AppPlatformGateState::Implemented
    );

    let by_id: BTreeMap<_, _> = manifest
        .capabilities()
        .iter()
        .map(|capability| (capability.id(), capability))
        .collect();

    let modes = [
        "application.chatbot",
        "application.chatflow",
        "application.classic-agent",
        "application.new-agent",
        "application.text-generator",
        "application.workflow",
    ];
    for mode in modes {
        let capability = by_id[mode];
        assert_eq!(capability.gate(), "APP0.4");
        assert_eq!(
            capability.availability(),
            AppPlatformCapabilityAvailability::Internal
        );
    }

    let deferred = [
        "publication.internal",
        "publication.mcp",
        "toolkit.acl-import-export",
        "toolkit.collaborative-revision",
        "toolkit.error-policy",
        "toolkit.global-discovery",
        "toolkit.hosted-mcp-facade",
        "toolkit.internal-invocation",
        "toolkit.moderation",
        "toolkit.new-agent-build-chat",
        "toolkit.new-agent-skill-files",
        "toolkit.node-test",
        "toolkit.snippets",
        "toolkit.stt",
        "toolkit.templates-catalog",
        "toolkit.tts",
        "toolkit.variable-inspection",
        "toolkit.version-control",
    ];
    for capability_id in deferred {
        let capability = by_id[capability_id];
        assert_eq!(capability.gate(), "APP0.4");
        assert_eq!(
            capability.availability(),
            AppPlatformCapabilityAvailability::Unavailable
        );
    }

    assert!(!manifest.parity_claim());
    assert_eq!(manifest.public_claim_gate(), "APP0.6");
}

#[test]
fn app05_production_foundation_claims_feedback_usage_and_defers_ops_monitoring() {
    let manifest = AppPlatformParityManifest::parse_acl(MANIFEST).expect("manifest");
    assert_eq!(
        manifest
            .gates()
            .iter()
            .find(|gate| gate.id() == "APP0.5")
            .expect("APP0.5 gate")
            .state(),
        AppPlatformGateState::Implemented
    );

    let by_id: BTreeMap<_, _> = manifest
        .capabilities()
        .iter()
        .map(|capability| (capability.id(), capability))
        .collect();

    for capability_id in ["monitoring.feedback-review", "monitoring.usage-cost"] {
        let capability = by_id[capability_id];
        assert_eq!(capability.gate(), "APP0.5");
        assert_eq!(
            capability.availability(),
            AppPlatformCapabilityAvailability::Internal
        );
    }

    let deferred = [
        "monitoring.alerts",
        "monitoring.latency-failure",
        "monitoring.retention-redaction",
        "monitoring.run-history",
        "monitoring.telemetry-export",
    ];
    for capability_id in deferred {
        let capability = by_id[capability_id];
        assert_eq!(capability.gate(), "APP0.5");
        assert_eq!(
            capability.availability(),
            AppPlatformCapabilityAvailability::Unavailable
        );
        assert_eq!(capability.owner(), "operations_telemetry");
    }

    assert!(!manifest.parity_claim());
    assert_eq!(manifest.public_claim_gate(), "APP0.6");
}

#[test]
fn app06_production_foundation_claims_custom_domain_and_isolation_quota() {
    let manifest = AppPlatformParityManifest::parse_acl(MANIFEST).expect("manifest");
    assert_eq!(
        manifest
            .gates()
            .iter()
            .find(|gate| gate.id() == "APP0.6")
            .expect("APP0.6 gate")
            .state(),
        AppPlatformGateState::Implemented
    );

    let by_id: BTreeMap<_, _> = manifest
        .capabilities()
        .iter()
        .map(|capability| (capability.id(), capability))
        .collect();

    for capability_id in [
        "enterprise.custom-domain-branding",
        "enterprise.isolation-quota-retention",
    ] {
        let capability = by_id[capability_id];
        assert_eq!(capability.gate(), "APP0.6");
        assert_eq!(
            capability.availability(),
            AppPlatformCapabilityAvailability::Internal
        );
    }

    // Remaining enterprise inventory stays on foreign gates; APP0.6 does not
    // own SSO/HA/BYOK/audit-security/external-identity close-out.
    let foreign_gate_enterprise = [
        ("enterprise.audit-security", "C0.5"),
        ("enterprise.byok-residency-airgap", "S0"),
        ("enterprise.external-identity", "C0.3"),
        ("enterprise.ha-disaster-recovery", "H0.5"),
        ("enterprise.saml-oidc-scim", "C0.5"),
    ];
    for (capability_id, gate_id) in foreign_gate_enterprise {
        let capability = by_id[capability_id];
        assert_eq!(capability.gate(), gate_id);
        assert_ne!(capability.gate(), "APP0.6");
    }

    assert!(!manifest.parity_claim());
    assert_eq!(manifest.public_claim_gate(), "APP0.6");
}

#[test]
fn c05_production_foundation_claims_oidc_and_audit_security() {
    let manifest = AppPlatformParityManifest::parse_acl(MANIFEST).expect("manifest");
    assert_eq!(
        manifest
            .gates()
            .iter()
            .find(|gate| gate.id() == "C0.5")
            .expect("C0.5 gate")
            .state(),
        AppPlatformGateState::Implemented
    );

    let by_id: BTreeMap<_, _> = manifest
        .capabilities()
        .iter()
        .map(|capability| (capability.id(), capability))
        .collect();

    for capability_id in ["enterprise.saml-oidc-scim", "enterprise.audit-security"] {
        let capability = by_id[capability_id];
        assert_eq!(capability.gate(), "C0.5");
        assert_eq!(
            capability.availability(),
            AppPlatformCapabilityAvailability::Internal
        );
    }

    assert!(!manifest.parity_claim());
    assert_eq!(manifest.public_claim_gate(), "APP0.6");
}

#[test]
fn c03_production_foundation_claims_orgs_and_external_identity() {
    let manifest = AppPlatformParityManifest::parse_acl(MANIFEST).expect("manifest");
    assert_eq!(
        manifest
            .gates()
            .iter()
            .find(|gate| gate.id() == "C0.3")
            .expect("C0.3 gate")
            .state(),
        AppPlatformGateState::Implemented
    );

    let by_id: BTreeMap<_, _> = manifest
        .capabilities()
        .iter()
        .map(|capability| (capability.id(), capability))
        .collect();

    for capability_id in [
        "enterprise.organizations-workspaces",
        "enterprise.external-identity",
    ] {
        let capability = by_id[capability_id];
        assert_eq!(capability.gate(), "C0.3");
        assert_eq!(
            capability.availability(),
            AppPlatformCapabilityAvailability::Internal
        );
    }

    assert!(!manifest.parity_claim());
    assert_eq!(manifest.public_claim_gate(), "APP0.6");
}

#[test]
fn w03_c1_claims_iteration_internal_without_closing_gate() {
    let manifest = AppPlatformParityManifest::parse_acl(MANIFEST).expect("manifest");
    assert_eq!(
        manifest
            .gates()
            .iter()
            .find(|gate| gate.id() == "W0.3")
            .expect("W0.3 gate")
            .state(),
        AppPlatformGateState::Implemented
    );

    let by_id: BTreeMap<_, _> = manifest
        .capabilities()
        .iter()
        .map(|capability| (capability.id(), capability))
        .collect();

    let capability = by_id["node.iteration"];
    assert_eq!(capability.gate(), "W0.3");
    assert_eq!(capability.owner(), "workflow");
    assert_eq!(
        capability.availability(),
        AppPlatformCapabilityAvailability::Internal
    );
    assert!(capability.evidence().iter().any(|item| {
        item.contains("0242-w03-iteration-internal-claim-path.md")
    }));

    assert!(!manifest.parity_claim());
    assert_eq!(manifest.public_claim_gate(), "APP0.6");
}

#[test]
fn w03_c2_claims_loop_internal_without_closing_gate() {
    let manifest = AppPlatformParityManifest::parse_acl(MANIFEST).expect("manifest");
    assert_eq!(
        manifest
            .gates()
            .iter()
            .find(|gate| gate.id() == "W0.3")
            .expect("W0.3 gate")
            .state(),
        AppPlatformGateState::Implemented
    );

    let by_id: BTreeMap<_, _> = manifest
        .capabilities()
        .iter()
        .map(|capability| (capability.id(), capability))
        .collect();

    let capability = by_id["node.loop"];
    assert_eq!(capability.gate(), "W0.3");
    assert_eq!(capability.owner(), "workflow");
    assert_eq!(
        capability.availability(),
        AppPlatformCapabilityAvailability::Internal
    );
    assert!(capability.evidence().iter().any(|item| {
        item.contains("0243-w03-loop-internal-claim-path.md")
    }));

    assert_eq!(
        by_id["node.iteration"].availability(),
        AppPlatformCapabilityAvailability::Internal
    );
    assert_eq!(
        by_id["node.answer"].availability(),
        AppPlatformCapabilityAvailability::Internal
    );

    assert!(!manifest.parity_claim());
    assert_eq!(manifest.public_claim_gate(), "APP0.6");
}

#[test]
fn w03_c3_claims_answer_internal_without_closing_gate() {
    let manifest = AppPlatformParityManifest::parse_acl(MANIFEST).expect("manifest");
    assert_eq!(
        manifest
            .gates()
            .iter()
            .find(|gate| gate.id() == "W0.3")
            .expect("W0.3 gate")
            .state(),
        AppPlatformGateState::Implemented
    );

    let by_id: BTreeMap<_, _> = manifest
        .capabilities()
        .iter()
        .map(|capability| (capability.id(), capability))
        .collect();

    let capability = by_id["node.answer"];
    assert_eq!(capability.gate(), "W0.3");
    assert_eq!(capability.owner(), "applications");
    assert!(
        capability
            .dependencies()
            .iter()
            .any(|dependency| dependency == "APP0.2")
    );
    assert_eq!(
        capability.availability(),
        AppPlatformCapabilityAvailability::Internal
    );
    assert!(capability.evidence().iter().any(|item| {
        item.contains("0244-w03-answer-internal-claim-path.md")
    }));

    assert!(!manifest.parity_claim());
    assert_eq!(manifest.public_claim_gate(), "APP0.6");
}


#[test]
fn w03_c4_production_foundation_closes_gate_without_public_claim() {
    let manifest = AppPlatformParityManifest::parse_acl(MANIFEST).expect("manifest");
    assert_eq!(
        manifest
            .gates()
            .iter()
            .find(|gate| gate.id() == "W0.3")
            .expect("W0.3 gate")
            .state(),
        AppPlatformGateState::Implemented
    );

    let w03: Vec<_> = manifest
        .capabilities()
        .iter()
        .filter(|capability| capability.gate() == "W0.3")
        .collect();
    assert_eq!(w03.len(), 10);
    assert!(w03.iter().all(|capability| {
        capability.availability() == AppPlatformCapabilityAvailability::Internal
    }));
    assert!(manifest
        .gates()
        .iter()
        .find(|gate| gate.id() == "W0.3")
        .expect("W0.3 gate")
        .evidence()
        .iter()
        .any(|item| item.contains("0245-w03-production-foundation.md")));

    assert!(!manifest.parity_claim());
    assert_eq!(manifest.public_claim_gate(), "APP0.6");
}


#[test]
fn aut05_production_foundation_closes_gate_without_inventing_dependents() {
    let manifest = AppPlatformParityManifest::parse_acl(MANIFEST).expect("manifest");
    assert_eq!(
        manifest
            .gates()
            .iter()
            .find(|gate| gate.id() == "AUT0.5")
            .expect("AUT0.5 gate")
            .state(),
        AppPlatformGateState::Implemented
    );
    assert!(manifest
        .gates()
        .iter()
        .find(|gate| gate.id() == "AUT0.5")
        .expect("AUT0.5 gate")
        .evidence()
        .iter()
        .any(|item| item.contains("0246-aut05-production-foundation.md")));

    // No capability is gated on AUT0.5; foreign dependents stay unavailable.
    assert!(manifest
        .capabilities()
        .iter()
        .filter(|capability| capability.gate() == "AUT0.5")
        .count()
        == 0);
    let by_id: BTreeMap<_, _> = manifest
        .capabilities()
        .iter()
        .map(|capability| (capability.id(), capability))
        .collect();
    assert_eq!(
        by_id["knowledge.datasource-web"].availability(),
        AppPlatformCapabilityAvailability::Unavailable
    );
    assert_eq!(
        by_id["toolkit.moderation"].availability(),
        AppPlatformCapabilityAvailability::Unavailable
    );
    assert_eq!(
        by_id["node.http-request"].availability(),
        AppPlatformCapabilityAvailability::Internal
    );

    assert!(!manifest.parity_claim());
    assert_eq!(manifest.public_claim_gate(), "APP0.6");
}


#[test]
fn w04_production_foundation_claims_http_request_and_defers_foreign_nodes() {
    let manifest = AppPlatformParityManifest::parse_acl(MANIFEST).expect("manifest");
    assert_eq!(
        manifest
            .gates()
            .iter()
            .find(|gate| gate.id() == "W0.4")
            .expect("W0.4 gate")
            .state(),
        AppPlatformGateState::Implemented
    );
    assert!(manifest
        .gates()
        .iter()
        .find(|gate| gate.id() == "W0.4")
        .expect("W0.4 gate")
        .evidence()
        .iter()
        .any(|item| item.contains("0247-w04-production-foundation.md")));

    let by_id: BTreeMap<_, _> = manifest
        .capabilities()
        .iter()
        .map(|capability| (capability.id(), capability))
        .collect();
    assert_eq!(
        by_id["node.http-request"].availability(),
        AppPlatformCapabilityAvailability::Internal
    );
    for deferred in [
        "node.agent",
        "node.code",
        "node.document-extractor",
        "node.knowledge-retrieval",
        "node.llm",
        "node.parameter-extractor",
        "node.question-classifier",
        "node.tool",
        "node.variable-assigner",
    ] {
        assert_eq!(
            by_id[deferred].availability(),
            AppPlatformCapabilityAvailability::Unavailable,
            "{deferred}"
        );
    }

    assert!(!manifest.parity_claim());
    assert_eq!(manifest.public_claim_gate(), "APP0.6");
}


#[test]
fn a05_production_foundation_closes_gate_without_external_provider_verification() {
    let manifest = AppPlatformParityManifest::parse_acl(MANIFEST).expect("manifest");
    let gate = manifest
        .gates()
        .iter()
        .find(|gate| gate.id() == "A0.5")
        .expect("A0.5 gate");
    assert_eq!(gate.state(), AppPlatformGateState::Implemented);
    assert!(gate
        .evidence()
        .iter()
        .any(|item| item.contains("0248-a05-production-foundation.md")));
    assert!(gate.evidence().iter().any(|item| {
        item.contains("skill_release_admission.rs")
            || item.contains("067_skill_workload_revision_bindings.sql")
    }));
    assert!(gate
        .evidence()
        .iter()
        .any(|item| item.contains("skill_lifecycle.rs")));

    // No app-platform capability invents an A0.5 owning gate on this close-out.
    assert!(
        manifest
            .capabilities()
            .iter()
            .all(|capability| capability.gate() != "A0.5"),
        "A0.5 must not invent capability ownership"
    );

    assert!(!manifest.parity_claim());
    assert_eq!(manifest.public_claim_gate(), "APP0.6");
}


#[test]
fn aut02_production_foundation_claims_webhook_trigger_internal() {
    let manifest = AppPlatformParityManifest::parse_acl(MANIFEST).expect("manifest");
    assert_eq!(
        manifest
            .gates()
            .iter()
            .find(|gate| gate.id() == "AUT0.2")
            .expect("AUT0.2 gate")
            .state(),
        AppPlatformGateState::Implemented
    );
    let by_id: BTreeMap<_, _> = manifest
        .capabilities()
        .iter()
        .map(|capability| (capability.id(), capability))
        .collect();
    assert_eq!(
        by_id["node.webhook-trigger"].availability(),
        AppPlatformCapabilityAvailability::Internal
    );
    assert_eq!(by_id["node.webhook-trigger"].gate(), "AUT0.2");
    assert_eq!(
        by_id["node.integration-trigger"].availability(),
        AppPlatformCapabilityAvailability::Unavailable
    );
    assert!(!manifest.parity_claim());
    assert_eq!(manifest.public_claim_gate(), "APP0.6");
}


#[test]
fn aut03_production_foundation_claims_schedule_trigger_internal() {
    let manifest = AppPlatformParityManifest::parse_acl(MANIFEST).expect("manifest");
    assert_eq!(
        manifest
            .gates()
            .iter()
            .find(|gate| gate.id() == "AUT0.3")
            .expect("AUT0.3 gate")
            .state(),
        AppPlatformGateState::Implemented
    );
    let by_id: BTreeMap<_, _> = manifest
        .capabilities()
        .iter()
        .map(|capability| (capability.id(), capability))
        .collect();
    assert_eq!(
        by_id["node.schedule-trigger"].availability(),
        AppPlatformCapabilityAvailability::Internal
    );
    assert_eq!(by_id["node.schedule-trigger"].gate(), "AUT0.3");
    assert_eq!(
        by_id["node.integration-trigger"].availability(),
        AppPlatformCapabilityAvailability::Unavailable
    );
    assert!(!manifest.parity_claim());
    assert_eq!(manifest.public_claim_gate(), "APP0.6");
}


#[test]
fn a13_production_foundation_closes_gate_without_inventing_capabilities() {
    let manifest = AppPlatformParityManifest::parse_acl(MANIFEST).expect("manifest");
    let gate = manifest
        .gates()
        .iter()
        .find(|gate| gate.id() == "A1.3")
        .expect("A1.3 gate");
    assert_eq!(gate.state(), AppPlatformGateState::Implemented);
    assert!(gate
        .evidence()
        .iter()
        .any(|item| item.contains("0252-a13-production-foundation.md")));
    assert!(gate.evidence().iter().any(|item| {
        item.contains("agent_provider_contract.rs")
            || item.contains("a3s-code-provider-profile.acl")
    }));
    assert!(
        manifest
            .capabilities()
            .iter()
            .all(|capability| capability.gate() != "A1.3"),
        "A1.3 must not invent capability ownership"
    );
    assert!(!manifest.parity_claim());
    assert_eq!(manifest.public_claim_gate(), "APP0.6");
}

#[test]
fn a14_production_foundation_closes_gate_without_inventing_capabilities() {
    let manifest = AppPlatformParityManifest::parse_acl(MANIFEST).expect("manifest");
    let gate = manifest
        .gates()
        .iter()
        .find(|gate| gate.id() == "A1.4")
        .expect("A1.4 gate");
    assert_eq!(gate.state(), AppPlatformGateState::Implemented);
    assert!(gate
        .evidence()
        .iter()
        .any(|item| item.contains("0253-a14-production-foundation.md")));
    assert!(gate.evidence().iter().any(|item| {
        item.contains("agent_provider_contract.rs")
            || item.contains("invocation.rs")
            || item.contains("agent_execution_flow/binding.rs")
    }));
    assert!(
        manifest
            .capabilities()
            .iter()
            .all(|capability| capability.gate() != "A1.4"),
        "A1.4 must not invent capability ownership"
    );
    assert_eq!(
        manifest
            .capabilities()
            .iter()
            .find(|capability| capability.id() == "toolkit.new-agent-build-chat")
            .expect("toolkit.new-agent-build-chat")
            .availability(),
        AppPlatformCapabilityAvailability::Unavailable
    );
    assert!(!manifest.parity_claim());
    assert_eq!(manifest.public_claim_gate(), "APP0.6");
}


#[test]
fn i02c_production_foundation_closes_gate_without_inventing_public_usage() {
    let manifest = AppPlatformParityManifest::parse_acl(MANIFEST).expect("manifest");
    let gate = manifest
        .gates()
        .iter()
        .find(|gate| gate.id() == "I0.2c")
        .expect("I0.2c gate");
    assert_eq!(gate.state(), AppPlatformGateState::Implemented);
    assert!(gate
        .evidence()
        .iter()
        .any(|item| item.contains("0254-i02c-production-foundation.md")));
    assert!(gate.evidence().iter().any(|item| {
        item.contains("0169-app05-usage-cost-showback-claim-path.md")
            || item.contains("usage_queries_controller.rs")
            || item.contains("monitoring_usage_cost_claim_path_tests.rs")
    }));
    let usage = manifest
        .capabilities()
        .iter()
        .find(|capability| capability.id() == "monitoring.usage-cost")
        .expect("monitoring.usage-cost");
    assert_eq!(
        usage.availability(),
        AppPlatformCapabilityAvailability::Internal
    );
    assert!(usage.dependencies().iter().any(|dep| dep == "I0.2c"));
    assert_eq!(
        manifest
            .gates()
            .iter()
            .find(|gate| gate.id() == "I0.2")
            .expect("I0.2")
            .state(),
        AppPlatformGateState::Planned
    );
    assert_eq!(
        manifest
            .gates()
            .iter()
            .find(|gate| gate.id() == "I0.6")
            .expect("I0.6")
            .state(),
        AppPlatformGateState::Planned
    );
    assert!(!manifest.parity_claim());
    assert_eq!(manifest.public_claim_gate(), "APP0.6");
}


#[test]
fn aut04_production_foundation_closes_gate_without_inventing_integration_trigger() {
    let manifest = AppPlatformParityManifest::parse_acl(MANIFEST).expect("manifest");
    let gate = manifest
        .gates()
        .iter()
        .find(|gate| gate.id() == "AUT0.4")
        .expect("AUT0.4 gate");
    assert_eq!(gate.state(), AppPlatformGateState::Implemented);
    assert!(gate
        .evidence()
        .iter()
        .any(|item| item.contains("0255-aut04-production-foundation.md")));
    assert!(gate.evidence().iter().any(|item| {
        item.contains("event_dispatch.rs")
            || item.contains("event_fanout.rs")
            || item.contains("event_consumer.rs")
            || item.contains("event_invocation.rs")
    }));
    assert_eq!(
        manifest
            .capabilities()
            .iter()
            .find(|capability| capability.id() == "node.integration-trigger")
            .expect("node.integration-trigger")
            .availability(),
        AppPlatformCapabilityAvailability::Unavailable
    );
    assert_eq!(
        manifest
            .capabilities()
            .iter()
            .find(|capability| capability.id() == "plugin.trigger")
            .expect("plugin.trigger")
            .availability(),
        AppPlatformCapabilityAvailability::Unavailable
    );
    assert_eq!(
        manifest
            .gates()
            .iter()
            .find(|gate| gate.id() == "U0.4")
            .expect("U0.4")
            .state(),
        AppPlatformGateState::Planned
    );
    assert!(!manifest.parity_claim());
    assert_eq!(manifest.public_claim_gate(), "APP0.6");
}

#[test]
fn h05_production_foundation_claims_ha_disaster_recovery() {
    let manifest = AppPlatformParityManifest::parse_acl(MANIFEST).expect("manifest");
    assert_eq!(
        manifest
            .gates()
            .iter()
            .find(|gate| gate.id() == "H0.5")
            .expect("H0.5 gate")
            .state(),
        AppPlatformGateState::Implemented
    );

    let by_id: BTreeMap<_, _> = manifest
        .capabilities()
        .iter()
        .map(|capability| (capability.id(), capability))
        .collect();

    let capability = by_id["enterprise.ha-disaster-recovery"];
    assert_eq!(capability.gate(), "H0.5");
    assert_eq!(
        capability.availability(),
        AppPlatformCapabilityAvailability::Internal
    );

    // S0 BYOK remains unavailable until a non-invented claim path exists.
    assert_eq!(
        by_id["enterprise.byok-residency-airgap"].availability(),
        AppPlatformCapabilityAvailability::Unavailable
    );
    assert_eq!(
        manifest
            .gates()
            .iter()
            .find(|gate| gate.id() == "S0")
            .expect("S0 gate")
            .state(),
        AppPlatformGateState::Planned
    );

    assert!(!manifest.parity_claim());
    assert_eq!(manifest.public_claim_gate(), "APP0.6");
}
