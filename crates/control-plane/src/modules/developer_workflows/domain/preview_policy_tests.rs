use super::{
    AcceptedPullRequestPreviewPolicyRevision, PullRequestPreviewPolicyContract,
    MAX_DEVELOPER_WORKFLOW_SAFE_INTEGER, PULL_REQUEST_PREVIEW_POLICY_SCHEMA,
};
use crate::modules::shared_kernel::domain::{EnvironmentId, PrincipalId};
use chrono::Utc;

const POLICY_FIXTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/p0.3/pull-request-preview-policy.acl"
));

#[test]
fn preview_policy_acl_is_canonical_closed_and_digest_locked() {
    let contract = PullRequestPreviewPolicyContract::parse_acl(POLICY_FIXTURE).expect("policy");
    assert_eq!(contract.schema(), PULL_REQUEST_PREVIEW_POLICY_SCHEMA);
    assert_eq!(
        contract.canonical_acl(),
        POLICY_FIXTURE.replace("\r\n", "\n")
    );
    assert_eq!(
        PullRequestPreviewPolicyContract::from_policy(contract.policy().clone())
            .expect("generated policy"),
        contract
    );
    assert_eq!(
        contract.digest().as_str(),
        "sha256:9f84a20d62d8afcdc909b74d19b4f87a428c55651cd9cc5cbf7ae4ff594a3e2f"
    );
    for forbidden in [
        "webhook_secret",
        "signature",
        "delivery_body",
        "credential",
        "checkout_path",
        "build_run_id",
        "route_id",
    ] {
        assert!(!contract.canonical_acl().contains(forbidden));
    }
}

#[test]
fn preview_policy_parser_rejects_unknown_noncanonical_and_drifted_storage() {
    let contract = PullRequestPreviewPolicyContract::parse_acl(POLICY_FIXTURE).expect("policy");
    let unknown = contract.canonical_acl().replacen(
        "  base_branch",
        "  provider_token = \"forbidden\"\n  base_branch",
        1,
    );
    assert!(PullRequestPreviewPolicyContract::parse_acl(&unknown).is_err());

    let noncanonical =
        contract
            .canonical_acl()
            .replacen("  lifetime_seconds", "    lifetime_seconds", 1);
    assert!(PullRequestPreviewPolicyContract::parse_acl(&noncanonical).is_err());
    assert!(PullRequestPreviewPolicyContract::restore(
        contract.canonical_acl(),
        &format!("sha256:{}", "f".repeat(64))
    )
    .is_err());
}

#[test]
fn accepted_policy_revision_has_deterministic_identity_and_exact_context() {
    let contract = PullRequestPreviewPolicyContract::parse_acl(POLICY_FIXTURE).expect("policy");
    let environment_id = EnvironmentId::new();
    let actor = PrincipalId::new();
    let accepted_at = Utc::now();
    let first = AcceptedPullRequestPreviewPolicyRevision::accept(
        environment_id,
        contract.clone(),
        1,
        actor,
        accepted_at,
    )
    .expect("accepted policy");
    let replay = AcceptedPullRequestPreviewPolicyRevision::accept(
        environment_id,
        contract,
        1,
        actor,
        accepted_at,
    )
    .expect("deterministic acceptance");
    assert_eq!(first, replay);
    assert_eq!(
        first.source_subscription_id,
        first.contract.policy().source_subscription_id
    );
    assert!(first.validate().is_ok());
}

#[test]
fn accepted_policy_revision_rejects_non_portable_revision_numbers() {
    let contract = PullRequestPreviewPolicyContract::parse_acl(POLICY_FIXTURE).expect("policy");
    let error = AcceptedPullRequestPreviewPolicyRevision::accept(
        EnvironmentId::new(),
        contract.clone(),
        MAX_DEVELOPER_WORKFLOW_SAFE_INTEGER + 1,
        PrincipalId::new(),
        Utc::now(),
    )
    .expect_err("non-portable revision number");
    assert!(error.contains("revision"));

    let mut revision = AcceptedPullRequestPreviewPolicyRevision::accept(
        EnvironmentId::new(),
        contract,
        1,
        PrincipalId::new(),
        Utc::now(),
    )
    .expect("accepted policy");
    revision.revision_number = MAX_DEVELOPER_WORKFLOW_SAFE_INTEGER + 1;
    assert!(revision.validate().is_err());
}
