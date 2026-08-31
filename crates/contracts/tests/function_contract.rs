use a3s_cloud_contracts::{
    FunctionEgressClassV1, FunctionFailureDispositionV1, FunctionInvocationAuthorityV1,
    FunctionInvocationFailureCodeV1, FunctionInvocationFailureV1, FunctionInvocationInputV1,
    FunctionInvocationParentKindV1, FunctionInvocationParentV1, FunctionInvocationPolicyV1,
    FunctionInvocationSlotV1, FunctionInvocationTargetV1, FunctionModeV1, FunctionOwnerV1,
    FunctionProfileV1, RuntimeIsolationLevel, FUNCTION_HOSTED_TASK_MAX_INPUT_BYTES,
};
use chrono::{DateTime, Duration, Utc};
use serde_json::json;
use sha2::{Digest, Sha256};
use uuid::Uuid;

const HOSTED_TASK: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/fn0.1/function-profile-hosted-task.acl"
));
const HOSTED_SERVICE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/fn0.1/function-profile-hosted-service.acl"
));
const EXTERNAL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/fn0.1/function-profile-external.acl"
));

#[test]
fn checked_in_profiles_freeze_the_closed_mode_and_owner_matrix() {
    for (source, expected_hash, mode, owner) in [
        (
            HOSTED_TASK,
            "5264781fcafb5789178adec165d39ec838ca9596ed59e543934956cad9f130cc",
            FunctionModeV1::HostedTask,
            FunctionOwnerV1::Executions,
        ),
        (
            HOSTED_SERVICE,
            "688fb79d12978ff2fac8c60fd718df4e010e3119957c40298ffe0ec92ffc6f7f",
            FunctionModeV1::HostedService,
            FunctionOwnerV1::Workloads,
        ),
        (
            EXTERNAL,
            "0949a9b30c7aacc6c53afac7b45a067ce9379a462cd8659288bfb580c5ac7012",
            FunctionModeV1::External,
            FunctionOwnerV1::Connectors,
        ),
    ] {
        assert_eq!(
            format!("{:x}", Sha256::digest(source.as_bytes())),
            expected_hash
        );
        let profile = FunctionProfileV1::parse_acl(source).expect("canonical Function profile");
        assert_eq!(profile.mode(), mode);
        assert_eq!(profile.owner(), owner);
        let normalized = source.replace("\r\n", "\n");
        assert_eq!(profile.canonical_acl(), normalized);
        assert!(profile.digest().starts_with("sha256:"));
        assert_eq!(
            FunctionProfileV1::restore(profile.canonical_acl(), profile.digest())
                .expect("restored profile"),
            profile
        );
    }
}

#[test]
fn profiles_fail_closed_on_foreign_state_and_mode_drift() {
    assert!(FunctionProfileV1::parse_acl(&HOSTED_TASK.replacen(
        "function_profile {",
        "function_profile  {",
        1,
    ))
    .is_err());
    assert!(FunctionProfileV1::parse_acl(&format!("{HOSTED_TASK}\n")).is_err());
    assert!(FunctionProfileV1::parse_acl(&HOSTED_TASK.replacen(
        "  mode = \"hosted_task\"",
        "  mode = \"external\"",
        1,
    ))
    .is_err());
    assert!(FunctionProfileV1::parse_acl(&HOSTED_TASK.replacen(
        "    timeout_ms = 900000",
        "    retry_count = 3\n    timeout_ms = 900000",
        1,
    ))
    .is_err());
    assert!(FunctionProfileV1::parse_acl(&HOSTED_TASK.replacen(
        "  schema = \"cloud.function.profile.v1\"",
        "  raw_credential = \"secret\"\n  schema = \"cloud.function.profile.v1\"",
        1,
    ))
    .is_err());
    assert!(FunctionProfileV1::parse_acl(&EXTERNAL.replacen(
        "  policy {",
        "  policy {\n    isolation = \"sandbox\"",
        1,
    ))
    .is_err());
    assert!(FunctionProfileV1::parse_acl(&EXTERNAL.replacen(
        "  security {",
        "  security {\n    secret \"provider-token\" {\n      secret_id = \"eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee\"\n      version = 1\n    }",
        1,
    ))
    .is_err());
    assert!(FunctionProfileV1::parse_acl(&HOSTED_TASK.replacen(
        "    max_input_bytes = 1048576",
        "    max_input_bytes = 67108865",
        1,
    ))
    .is_err());
}

#[test]
fn invocation_authority_binds_caller_target_input_and_policy() {
    let profile = FunctionProfileV1::parse_acl(HOSTED_TASK).expect("hosted Task profile");
    let invocation = invocation(&profile);
    invocation
        .validate_for_profile(&profile)
        .expect("admitted invocation");
    assert!(invocation
        .digest()
        .expect("invocation digest")
        .starts_with("sha256:"));

    let mut wrong_target = invocation.clone();
    wrong_target.target.asset_release_id = Uuid::new_v4();
    assert!(wrong_target.validate_for_profile(&profile).is_err());

    let mut wrong_egress = invocation.clone();
    wrong_egress.policy.egress_class = FunctionEgressClassV1::Public;
    assert!(wrong_egress.validate_for_profile(&profile).is_err());

    let mut late = invocation.clone();
    late.policy.deadline_at = late.policy.requested_at + Duration::hours(1);
    assert!(late.validate_for_profile(&profile).is_err());

    let mut oversized = invocation;
    oversized.input = FunctionInvocationInputV1::immutable_object(
        "function-inputs",
        "organizations/11111111/invocations/oversized",
        "application/json",
        digest('f'),
        FUNCTION_HOSTED_TASK_MAX_INPUT_BYTES + 1,
    )
    .expect("bounded object reference");
    assert!(oversized.validate_for_profile(&profile).is_err());
}

#[test]
fn inline_input_is_digest_bound_and_wire_shape_rejects_unknown_fields() {
    let input = FunctionInvocationInputV1::inline_json(
        "application/json",
        json!({"city": "Shanghai", "units": "metric"}),
    )
    .expect("inline input");
    assert!(input.digest().starts_with("sha256:"));
    assert!(input.size_bytes() > 0);

    let profile = FunctionProfileV1::parse_acl(HOSTED_TASK).expect("profile");
    let wire_invocation = invocation(&profile);
    let mut wire = serde_json::to_value(&wire_invocation).expect("wire value");
    wire.as_object_mut()
        .expect("wire object")
        .insert("retryCount".into(), json!(3));
    assert!(serde_json::from_value::<FunctionInvocationAuthorityV1>(wire).is_err());

    let mut drifted = input;
    let FunctionInvocationInputV1::InlineJson {
        digest: stored_digest,
        ..
    } = &mut drifted
    else {
        unreachable!()
    };
    *stored_digest = digest('0');
    let mut invocation = invocation(&profile);
    invocation.input = drifted;
    assert!(invocation.validate().is_err());
}

#[test]
fn failures_preserve_owner_and_external_indeterminate_semantics() {
    let external = FunctionInvocationFailureV1 {
        schema: FunctionInvocationFailureV1::SCHEMA.into(),
        code: FunctionInvocationFailureCodeV1::ExternalOutcomeIndeterminate,
        owner: FunctionOwnerV1::Connectors,
        owner_reference_id: Some(Uuid::new_v4()),
        owner_evidence_digest: Some(digest('e')),
        message: "external Connector outcome is indeterminate".into(),
    };
    external
        .validate_for_mode(FunctionModeV1::External)
        .expect("external indeterminate failure");
    assert_eq!(
        external.disposition(),
        FunctionFailureDispositionV1::Indeterminate
    );
    assert!(external
        .validate_for_mode(FunctionModeV1::HostedTask)
        .is_err());

    let limited = FunctionInvocationFailureV1 {
        schema: FunctionInvocationFailureV1::SCHEMA.into(),
        code: FunctionInvocationFailureCodeV1::ConcurrencyLimited,
        owner: FunctionOwnerV1::Workloads,
        owner_reference_id: None,
        owner_evidence_digest: None,
        message: "capacity is currently unavailable".into(),
    };
    limited
        .validate_for_mode(FunctionModeV1::HostedService)
        .expect("caller-policy failure");
    assert_eq!(
        limited.disposition(),
        FunctionFailureDispositionV1::CallerPolicy
    );
}

#[test]
fn function_contract_types_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}

    assert_send_sync::<FunctionProfileV1>();
    assert_send_sync::<FunctionInvocationAuthorityV1>();
    assert_send_sync::<FunctionInvocationFailureV1>();
}

fn invocation(profile: &FunctionProfileV1) -> FunctionInvocationAuthorityV1 {
    let requested_at = timestamp("2026-08-31T00:00:00Z");
    FunctionInvocationAuthorityV1 {
        schema: FunctionInvocationAuthorityV1::SCHEMA.into(),
        invocation_id: Uuid::parse_str("12345678-1234-4234-8234-123456789abc")
            .expect("invocation ID"),
        organization_id: profile.spec().organization_id,
        project_id: Uuid::parse_str("23456789-2345-4345-8345-23456789abcd").expect("project ID"),
        environment_id: Uuid::parse_str("3456789a-3456-4456-8456-3456789abcde")
            .expect("environment ID"),
        parent: FunctionInvocationParentV1 {
            kind: FunctionInvocationParentKindV1::Workflow,
            id: Uuid::parse_str("456789ab-4567-4567-8567-456789abcdef").expect("parent ID"),
            revision_digest: digest('1'),
        },
        slot: FunctionInvocationSlotV1 {
            name: "step.weather".into(),
            attempt: 1,
        },
        target: FunctionInvocationTargetV1 {
            asset_id: profile.spec().asset_id,
            asset_release_id: profile.spec().asset_release_id,
            profile_digest: profile.digest().into(),
        },
        input: FunctionInvocationInputV1::inline_json(
            "application/json",
            json!({"city": "Shanghai"}),
        )
        .expect("input"),
        policy: FunctionInvocationPolicyV1 {
            requested_at,
            deadline_at: requested_at + Duration::seconds(30),
            idempotency_key: "workflow-run:step.weather:attempt:1".into(),
            authorization_digest: digest('2'),
            egress_class: profile.spec().security.egress_class,
        },
    }
}

fn digest(character: char) -> String {
    format!("sha256:{}", character.to_string().repeat(64))
}

fn timestamp(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .expect("timestamp")
        .with_timezone(&Utc)
}

#[allow(dead_code)]
fn _runtime_isolation_is_the_shared_contract(value: RuntimeIsolationLevel) -> &'static str {
    match value {
        RuntimeIsolationLevel::Process => "process",
        RuntimeIsolationLevel::Container => "container",
        RuntimeIsolationLevel::Sandbox => "sandbox",
        RuntimeIsolationLevel::Confidential => "confidential",
    }
}
