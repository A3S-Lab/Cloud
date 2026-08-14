use a3s_cloud_contracts::{
    AppPlatformParityManifest, WorkflowNodeExecutionClass, WorkflowNodeKind, WorkflowNodeProfiles,
};
use std::collections::BTreeMap;

const MANIFEST: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/app-platform/v1/parity-manifest.acl"
));
const PROFILES: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/app-platform/v1/workflow-node-profiles.acl"
));

#[test]
fn checked_in_profiles_exactly_enrich_the_twenty_three_node_inventory() {
    let manifest = AppPlatformParityManifest::parse_acl(MANIFEST).expect("manifest");
    let profiles = WorkflowNodeProfiles::parse_acl(PROFILES).expect("profiles");
    profiles.validate_manifest(&manifest).expect("coverage");

    assert_eq!(profiles.profiles().len(), 23);
    assert_eq!(profiles.parity_manifest_digest(), manifest.digest());
    assert!(profiles.digest().starts_with("sha256:"));
    assert_eq!(profiles.canonical_acl(), PROFILES.replace("\r\n", "\n"));
    assert_eq!(
        WorkflowNodeProfiles::restore(PROFILES, profiles.digest()).expect("restored"),
        profiles
    );

    let by_id = profiles
        .profiles()
        .iter()
        .map(|profile| (profile.capability_id(), profile))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(
        by_id["node.agent"].semantic_profiles(),
        ["agent.classic", "agent.release"]
    );
    assert_eq!(
        by_id["node.integration-trigger"].execution_class(),
        WorkflowNodeExecutionClass::InvocationOnly
    );
    assert_eq!(by_id["node.integration-trigger"].kind(), None);
    assert_eq!(
        by_id["node.iteration"].kind(),
        Some(WorkflowNodeKind::Subworkflow)
    );
    assert_eq!(by_id["node.code"].kind(), Some(WorkflowNodeKind::Execution));
}

#[test]
fn profiles_fail_closed_on_manifest_or_semantic_drift() {
    let manifest = AppPlatformParityManifest::parse_acl(MANIFEST).expect("manifest");
    let profiles = WorkflowNodeProfiles::parse_acl(PROFILES).expect("profiles");

    let stale_manifest = MANIFEST.replacen("owner = \"executions\"", "owner = \"workflow\"", 1);
    let stale_manifest =
        AppPlatformParityManifest::parse_acl(&stale_manifest).expect("stale manifest");
    assert!(profiles.validate_manifest(&stale_manifest).is_err());

    let rebound = PROFILES.replacen(manifest.digest(), stale_manifest.digest(), 1);
    let rebound = WorkflowNodeProfiles::parse_acl(&rebound).expect("rebound profiles");
    let error = rebound
        .validate_manifest(&stale_manifest)
        .expect_err("owner drift");
    assert!(error.contains("conflicts with owner"), "{error}");

    let duplicate = PROFILES.replacen("model.question-classifier", "model.llm", 1);
    assert!(WorkflowNodeProfiles::parse_acl(&duplicate).is_err());

    let invocation_with_kind = PROFILES.replacen(
        "  node \"node.integration-trigger\" {\n    execution_class = \"invocation_only\"\n",
        "  node \"node.integration-trigger\" {\n    execution_class = \"invocation_only\"\n    kind = \"input\"\n",
        1,
    );
    assert!(WorkflowNodeProfiles::parse_acl(&invocation_with_kind).is_err());

    let unknown = PROFILES.replacen(
        "  revision = \"1.0.0\"\n",
        "  legacy = true\n  revision = \"1.0.0\"\n",
        1,
    );
    assert!(WorkflowNodeProfiles::parse_acl(&unknown).is_err());
    assert!(WorkflowNodeProfiles::parse_acl(&format!("\n{PROFILES}")).is_err());
}

#[test]
fn profile_contracts_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<WorkflowNodeProfiles>();
}
