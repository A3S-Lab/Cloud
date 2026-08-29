use super::developer_workflow_components::{canonical_acl_schema, object_schema, schema_ref};
use super::workflow_components::{
    digest_schema, nullable_uuid_schema, revision_number_schema, timestamp_schema, uuid_schema,
};
use crate::modules::identity::domain::value_objects::{
    PlatformPermission, PlatformRole, PlatformRolePolicyContract, TenantNotificationRequirement,
    TenantSupportApprovalRequirement, TenantSupportGrantContract, TenantSupportGrantContractSpec,
    TenantSupportGrantMode, TenantSupportPermission, TrustDomainContract, TrustDomainContractSpec,
    TrustDomainName, WorkloadIdentityAudience, WorkloadIdentityFormat,
    WorkloadIdentityPolicyContract, WorkloadIdentityPolicySpec, WorkloadIdentityRevocationMode,
    WorkloadProductRole, PLATFORM_ROLE_POLICY_MAX_ACL_BYTES, TENANT_SUPPORT_GRANT_MAX_ACL_BYTES,
    TRUST_DOMAIN_CONTRACT_MAX_ACL_BYTES, WORKLOAD_IDENTITY_POLICY_MAX_ACL_BYTES,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, InstallationId, NodePoolId, OrganizationId, PlatformRolePolicyId, PrincipalId,
    ProjectId, ScopeContext, Sha256Digest, TenantSupportGrantId, TrustDomainId,
    TrustDomainRevisionId, WorkloadId, WorkloadIdentityPolicyId, WorkloadRevisionId,
};
use a3s_cloud_contracts::{RuntimeIsolationLevel, RuntimeUnitClass};
use chrono::{Duration, TimeZone, Utc};
use serde_json::{json, Map, Value};
use uuid::Uuid;

const MAXIMUM_JSON_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

pub(super) const PRIVILEGED_MANAGEMENT_SUCCESS_SCHEMA_BINDINGS: &[(&str, &str)] = &[
    ("PlatformRolePolicySuccessResponse", "PlatformRolePolicy"),
    (
        "PlatformRolePolicyMutationSuccessResponse",
        "PlatformRolePolicyMutation",
    ),
    ("PlatformRoleBindingSuccessResponse", "PlatformRoleBinding"),
    (
        "PlatformRoleBindingMutationSuccessResponse",
        "PlatformRoleBindingMutation",
    ),
    ("TenantSupportGrantSuccessResponse", "TenantSupportGrant"),
    (
        "TenantSupportGrantProposalMutationSuccessResponse",
        "TenantSupportGrantProposalMutation",
    ),
    (
        "TenantSupportGrantApprovalMutationSuccessResponse",
        "TenantSupportGrantApprovalMutation",
    ),
    (
        "TenantSupportGrantMutationSuccessResponse",
        "TenantSupportGrantMutation",
    ),
    ("TrustDomainRevisionSuccessResponse", "TrustDomainRevision"),
    (
        "TrustDomainRevisionListSuccessResponse",
        "TrustDomainRevisionList",
    ),
    (
        "TrustDomainRevisionMutationSuccessResponse",
        "TrustDomainRevisionMutation",
    ),
    (
        "WorkloadIdentityPolicyRevisionSuccessResponse",
        "WorkloadIdentityPolicyRevision",
    ),
    (
        "WorkloadIdentityPolicyRevisionListSuccessResponse",
        "WorkloadIdentityPolicyRevisionList",
    ),
    (
        "WorkloadIdentityPolicyRevisionMutationSuccessResponse",
        "WorkloadIdentityPolicyRevisionMutation",
    ),
];

pub(super) const PRIVILEGED_MANAGEMENT_SUCCESS_RESPONSE_BINDINGS: &[(&str, u16, &str)] = &[
    (
        "PlatformRolePolicySuccess200",
        200,
        "PlatformRolePolicySuccessResponse",
    ),
    (
        "PlatformRolePolicyMutationSuccess200",
        200,
        "PlatformRolePolicyMutationSuccessResponse",
    ),
    (
        "PlatformRoleBindingSuccess200",
        200,
        "PlatformRoleBindingSuccessResponse",
    ),
    (
        "PlatformRoleBindingMutationSuccess200",
        200,
        "PlatformRoleBindingMutationSuccessResponse",
    ),
    (
        "TenantSupportGrantSuccess200",
        200,
        "TenantSupportGrantSuccessResponse",
    ),
    (
        "TenantSupportGrantProposalMutationSuccess200",
        200,
        "TenantSupportGrantProposalMutationSuccessResponse",
    ),
    (
        "TenantSupportGrantApprovalMutationSuccess200",
        200,
        "TenantSupportGrantApprovalMutationSuccessResponse",
    ),
    (
        "TenantSupportGrantMutationSuccess200",
        200,
        "TenantSupportGrantMutationSuccessResponse",
    ),
    (
        "TrustDomainRevisionSuccess200",
        200,
        "TrustDomainRevisionSuccessResponse",
    ),
    (
        "TrustDomainRevisionListSuccess200",
        200,
        "TrustDomainRevisionListSuccessResponse",
    ),
    (
        "TrustDomainRevisionMutationSuccess200",
        200,
        "TrustDomainRevisionMutationSuccessResponse",
    ),
    (
        "WorkloadIdentityPolicyRevisionSuccess200",
        200,
        "WorkloadIdentityPolicyRevisionSuccessResponse",
    ),
    (
        "WorkloadIdentityPolicyRevisionListSuccess200",
        200,
        "WorkloadIdentityPolicyRevisionListSuccessResponse",
    ),
    (
        "WorkloadIdentityPolicyRevisionMutationSuccess200",
        200,
        "WorkloadIdentityPolicyRevisionMutationSuccessResponse",
    ),
];

pub(super) fn install_privileged_management_component_schemas(schemas: &mut Map<String, Value>) {
    for (name, schema) in [
        ("PlatformRolePermission", platform_role_permission_schema()),
        ("PlatformRolePolicy", platform_role_policy_schema()),
        (
            "PlatformRolePolicyMutation",
            with_replay(platform_role_policy_schema()),
        ),
        ("PlatformRoleBinding", platform_role_binding_schema()),
        (
            "PlatformRoleBindingMutation",
            with_replay(platform_role_binding_schema()),
        ),
        ("DecisionEvidence", decision_evidence_schema()),
        ("TenantSupportScope", tenant_support_scope_schema()),
        (
            "TenantSupportGrantProposal",
            tenant_support_grant_proposal_schema(),
        ),
        (
            "TenantSupportGrantApproval",
            tenant_support_grant_approval_schema(),
        ),
        (
            "TenantSupportGrantLifecycle",
            tenant_support_grant_lifecycle_schema(),
        ),
        ("TenantSupportGrant", tenant_support_grant_schema()),
        (
            "TenantSupportGrantProposalMutation",
            tenant_support_grant_proposal_mutation_schema(),
        ),
        (
            "TenantSupportGrantApprovalOutcome",
            tenant_support_grant_approval_outcome_schema(),
        ),
        (
            "TenantSupportGrantApprovalMutation",
            tenant_support_grant_approval_mutation_schema(),
        ),
        (
            "TenantSupportGrantMutation",
            tenant_support_grant_mutation_schema(),
        ),
        ("TrustDomainRevision", trust_domain_revision_schema()),
        (
            "TrustDomainRevisionList",
            revision_list_schema("TrustDomainRevision"),
        ),
        (
            "TrustDomainRevisionMutation",
            with_replay(trust_domain_revision_schema()),
        ),
        (
            "WorkloadIdentityPolicyRevision",
            workload_identity_policy_revision_schema(),
        ),
        (
            "WorkloadIdentityPolicyRevisionList",
            revision_list_schema("WorkloadIdentityPolicyRevision"),
        ),
        (
            "WorkloadIdentityPolicyRevisionMutation",
            with_replay(workload_identity_policy_revision_schema()),
        ),
    ] {
        schemas.insert(name.into(), schema);
    }
}

pub(super) fn accept_platform_role_policy_request_schema() -> Value {
    let example = platform_role_policy_acl_example();
    object_schema(
        &[
            "canonicalAcl",
            "revisionNumber",
            "expectedCurrentRevisionId",
        ],
        json!({
            "canonicalAcl": canonical_acl_schema(PLATFORM_ROLE_POLICY_MAX_ACL_BYTES, &example),
            "revisionNumber": revision_number_schema(),
            "expectedCurrentRevisionId": uuid_schema()
        }),
    )
}

pub(super) fn create_platform_role_binding_request_schema() -> Value {
    object_schema(
        &["principalId", "role", "expectedPolicyRevisionId"],
        json!({
            "principalId": uuid_schema(),
            "role": platform_role_schema(),
            "expectedPolicyRevisionId": uuid_schema()
        }),
    )
}

pub(super) fn change_platform_role_binding_request_schema() -> Value {
    object_schema(
        &["role", "expectedVersion", "expectedPolicyRevisionId"],
        json!({
            "role": platform_role_schema(),
            "expectedVersion": positive_version_schema(),
            "expectedPolicyRevisionId": uuid_schema()
        }),
    )
}

pub(super) fn expected_version_request_schema() -> Value {
    object_schema(
        &["expectedVersion"],
        json!({ "expectedVersion": positive_version_schema() }),
    )
}

pub(super) fn propose_tenant_support_grant_request_schema() -> Value {
    let example = tenant_support_grant_acl_example();
    object_schema(
        &["canonicalAcl"],
        json!({
            "canonicalAcl": canonical_acl_schema(TENANT_SUPPORT_GRANT_MAX_ACL_BYTES, &example)
        }),
    )
}

pub(super) fn approve_tenant_support_grant_request_schema() -> Value {
    object_schema(
        &["expectedContractDigest"],
        json!({ "expectedContractDigest": digest_schema() }),
    )
}

pub(super) fn accept_trust_domain_revision_request_schema() -> Value {
    object_schema(
        &["canonicalAcl", "revisionNumber"],
        json!({
            "canonicalAcl": canonical_acl_schema(
                TRUST_DOMAIN_CONTRACT_MAX_ACL_BYTES,
                &trust_domain_acl_example(),
            ),
            "revisionNumber": revision_number_schema(),
            "expectedPreviousRevisionId": nullable_uuid_schema()
        }),
    )
}

pub(super) fn accept_workload_identity_policy_revision_request_schema() -> Value {
    object_schema(
        &["canonicalAcl", "revisionNumber"],
        json!({
            "canonicalAcl": canonical_acl_schema(
                WORKLOAD_IDENTITY_POLICY_MAX_ACL_BYTES,
                &workload_identity_policy_acl_example(),
            ),
            "revisionNumber": revision_number_schema(),
            "expectedPreviousRevisionId": nullable_uuid_schema()
        }),
    )
}

fn platform_role_permission_schema() -> Value {
    object_schema(
        &["role", "permissions"],
        json!({
            "role": platform_role_schema(),
            "permissions": {
                "type": "array",
                "minItems": 1,
                "maxItems": PlatformPermission::ALL.len(),
                "uniqueItems": true,
                "items": platform_permission_schema()
            }
        }),
    )
}

fn platform_role_policy_schema() -> Value {
    object_schema(
        &[
            "id",
            "installationId",
            "policyId",
            "revisionNumber",
            "canonicalAcl",
            "digest",
            "rolePermissions",
            "acceptedBy",
            "acceptedAt",
        ],
        json!({
            "id": uuid_schema(),
            "installationId": uuid_schema(),
            "policyId": uuid_schema(),
            "revisionNumber": revision_number_schema(),
            "canonicalAcl": canonical_acl_schema(
                PLATFORM_ROLE_POLICY_MAX_ACL_BYTES,
                &platform_role_policy_acl_example(),
            ),
            "digest": digest_schema(),
            "rolePermissions": {
                "type": "array",
                "minItems": PlatformRole::ALL.len(),
                "maxItems": PlatformRole::ALL.len(),
                "uniqueItems": true,
                "items": schema_ref("PlatformRolePermission")
            },
            "acceptedBy": uuid_schema(),
            "acceptedAt": timestamp_schema()
        }),
    )
}

fn platform_role_binding_schema() -> Value {
    object_schema(
        &[
            "id",
            "installationId",
            "principalId",
            "role",
            "aggregateVersion",
            "createdBy",
            "updatedBy",
            "createdAt",
            "updatedAt",
            "revokedAt",
        ],
        json!({
            "id": uuid_schema(),
            "installationId": uuid_schema(),
            "principalId": uuid_schema(),
            "role": platform_role_schema(),
            "aggregateVersion": positive_version_schema(),
            "createdBy": uuid_schema(),
            "updatedBy": uuid_schema(),
            "createdAt": timestamp_schema(),
            "updatedAt": timestamp_schema(),
            "revokedAt": nullable_timestamp_schema()
        }),
    )
}

fn decision_evidence_schema() -> Value {
    object_schema(
        &["id", "digest"],
        json!({
            "id": {
                "type": "string",
                "minLength": 1,
                "maxLength": 512,
                "pattern": "^[^\\u0000\\r\\n]+$"
            },
            "digest": digest_schema()
        }),
    )
}

fn tenant_support_scope_schema() -> Value {
    object_schema(
        &[
            "kind",
            "installationId",
            "organizationId",
            "projectId",
            "environmentId",
        ],
        json!({
            "kind": { "type": "string", "enum": ["organization", "project", "environment"] },
            "installationId": uuid_schema(),
            "organizationId": nullable_uuid_schema(),
            "projectId": nullable_uuid_schema(),
            "environmentId": nullable_uuid_schema()
        }),
    )
}

fn tenant_support_grant_proposal_schema() -> Value {
    object_schema(
        &[
            "id",
            "principalId",
            "scope",
            "permissions",
            "caseReference",
            "justificationDigest",
            "mode",
            "approvalRequirement",
            "approverIds",
            "tenantNotification",
            "securityAlertRequired",
            "postIncidentReviewRequired",
            "startsAt",
            "expiresAt",
            "canonicalAcl",
            "contractDigest",
            "requestedBy",
            "authentication",
            "requestedAt",
        ],
        json!({
            "id": uuid_schema(),
            "principalId": uuid_schema(),
            "scope": schema_ref("TenantSupportScope"),
            "permissions": {
                "type": "array",
                "minItems": 1,
                "maxItems": TenantSupportPermission::ALL.len(),
                "uniqueItems": true,
                "items": tenant_support_permission_schema()
            },
            "caseReference": {
                "type": "string",
                "minLength": 1,
                "maxLength": 128,
                "pattern": "^[A-Za-z0-9._:/-]+$"
            },
            "justificationDigest": digest_schema(),
            "mode": { "type": "string", "enum": ["standard", "break_glass"] },
            "approvalRequirement": { "type": "string", "enum": ["single", "dual"] },
            "approverIds": {
                "type": "array",
                "minItems": 1,
                "maxItems": 2,
                "uniqueItems": true,
                "items": uuid_schema()
            },
            "tenantNotification": { "type": "string", "enum": ["required", "policy_exempt"] },
            "securityAlertRequired": { "type": "boolean" },
            "postIncidentReviewRequired": { "type": "boolean" },
            "startsAt": timestamp_schema(),
            "expiresAt": timestamp_schema(),
            "canonicalAcl": canonical_acl_schema(
                TENANT_SUPPORT_GRANT_MAX_ACL_BYTES,
                &tenant_support_grant_acl_example(),
            ),
            "contractDigest": digest_schema(),
            "requestedBy": uuid_schema(),
            "authentication": schema_ref("DecisionEvidence"),
            "requestedAt": timestamp_schema()
        }),
    )
}

fn tenant_support_grant_approval_schema() -> Value {
    object_schema(
        &[
            "grantId",
            "contractDigest",
            "approverId",
            "authentication",
            "policyRevisionId",
            "policyDigest",
            "bindingId",
            "bindingVersion",
            "approvedAt",
            "digest",
        ],
        json!({
            "grantId": uuid_schema(),
            "contractDigest": digest_schema(),
            "approverId": uuid_schema(),
            "authentication": schema_ref("DecisionEvidence"),
            "policyRevisionId": uuid_schema(),
            "policyDigest": digest_schema(),
            "bindingId": uuid_schema(),
            "bindingVersion": positive_version_schema(),
            "approvedAt": timestamp_schema(),
            "digest": digest_schema()
        }),
    )
}

fn tenant_support_grant_lifecycle_schema() -> Value {
    object_schema(
        &[
            "id",
            "aggregateVersion",
            "revocationGeneration",
            "acceptedAt",
            "revokedAt",
            "revokedBy",
        ],
        json!({
            "id": uuid_schema(),
            "aggregateVersion": positive_version_schema(),
            "revocationGeneration": non_negative_version_schema(),
            "acceptedAt": timestamp_schema(),
            "revokedAt": nullable_timestamp_schema(),
            "revokedBy": nullable_uuid_schema()
        }),
    )
}

fn tenant_support_grant_schema() -> Value {
    object_schema(
        &["proposal", "approvals", "grant"],
        json!({
            "proposal": schema_ref("TenantSupportGrantProposal"),
            "approvals": {
                "type": "array",
                "maxItems": 2,
                "uniqueItems": true,
                "items": schema_ref("TenantSupportGrantApproval")
            },
            "grant": nullable_schema_ref("TenantSupportGrantLifecycle")
        }),
    )
}

fn tenant_support_grant_proposal_mutation_schema() -> Value {
    object_schema(
        &["proposal", "replayed"],
        json!({
            "proposal": schema_ref("TenantSupportGrantProposal"),
            "replayed": { "type": "boolean" }
        }),
    )
}

fn tenant_support_grant_approval_outcome_schema() -> Value {
    object_schema(
        &["proposal", "approval", "grant"],
        json!({
            "proposal": schema_ref("TenantSupportGrantProposal"),
            "approval": schema_ref("TenantSupportGrantApproval"),
            "grant": nullable_schema_ref("TenantSupportGrantLifecycle")
        }),
    )
}

fn tenant_support_grant_approval_mutation_schema() -> Value {
    object_schema(
        &["outcome", "replayed"],
        json!({
            "outcome": schema_ref("TenantSupportGrantApprovalOutcome"),
            "replayed": { "type": "boolean" }
        }),
    )
}

fn tenant_support_grant_mutation_schema() -> Value {
    object_schema(
        &["grant", "replayed"],
        json!({
            "grant": schema_ref("TenantSupportGrantLifecycle"),
            "replayed": { "type": "boolean" }
        }),
    )
}

fn trust_domain_revision_schema() -> Value {
    object_schema(
        &[
            "installationId",
            "trustDomainId",
            "revisionId",
            "revisionNumber",
            "name",
            "canonicalAcl",
            "digest",
            "acceptedBy",
            "acceptedAt",
        ],
        json!({
            "installationId": uuid_schema(),
            "trustDomainId": uuid_schema(),
            "revisionId": uuid_schema(),
            "revisionNumber": revision_number_schema(),
            "name": {
                "type": "string",
                "minLength": 1,
                "maxLength": 253,
                "pattern": "^[a-z0-9](?:[a-z0-9.-]*[a-z0-9])?$"
            },
            "canonicalAcl": canonical_acl_schema(
                TRUST_DOMAIN_CONTRACT_MAX_ACL_BYTES,
                &trust_domain_acl_example(),
            ),
            "digest": digest_schema(),
            "acceptedBy": uuid_schema(),
            "acceptedAt": timestamp_schema()
        }),
    )
}

fn workload_identity_policy_revision_schema() -> Value {
    object_schema(
        &[
            "installationId",
            "organizationId",
            "projectId",
            "environmentId",
            "policyId",
            "revisionId",
            "revisionNumber",
            "trustDomainId",
            "trustDomainRevisionId",
            "workloadId",
            "workloadRevisionId",
            "nodePoolId",
            "canonicalAcl",
            "digest",
            "acceptedBy",
            "acceptedAt",
        ],
        json!({
            "installationId": uuid_schema(),
            "organizationId": uuid_schema(),
            "projectId": uuid_schema(),
            "environmentId": uuid_schema(),
            "policyId": uuid_schema(),
            "revisionId": uuid_schema(),
            "revisionNumber": revision_number_schema(),
            "trustDomainId": uuid_schema(),
            "trustDomainRevisionId": uuid_schema(),
            "workloadId": uuid_schema(),
            "workloadRevisionId": uuid_schema(),
            "nodePoolId": uuid_schema(),
            "canonicalAcl": canonical_acl_schema(
                WORKLOAD_IDENTITY_POLICY_MAX_ACL_BYTES,
                &workload_identity_policy_acl_example(),
            ),
            "digest": digest_schema(),
            "acceptedBy": uuid_schema(),
            "acceptedAt": timestamp_schema()
        }),
    )
}

fn revision_list_schema(item: &str) -> Value {
    json!({
        "type": "array",
        "maxItems": crate::modules::identity::domain::repositories::MAX_WORKLOAD_IDENTITY_REVISIONS_PAGE,
        "items": schema_ref(item)
    })
}

fn with_replay(mut schema: Value) -> Value {
    schema["required"]
        .as_array_mut()
        .expect("closed mutation schema required fields")
        .push(json!("replayed"));
    schema["properties"]
        .as_object_mut()
        .expect("closed mutation schema properties")
        .insert("replayed".into(), json!({ "type": "boolean" }));
    schema
}

fn platform_role_schema() -> Value {
    json!({
        "type": "string",
        "enum": PlatformRole::ALL.map(PlatformRole::as_str)
    })
}

fn platform_permission_schema() -> Value {
    let permissions = PlatformPermission::ALL
        .into_iter()
        .map(PlatformPermission::as_str)
        .collect::<Vec<_>>();
    json!({
        "type": "string",
        "enum": permissions
    })
}

fn tenant_support_permission_schema() -> Value {
    json!({
        "type": "string",
        "enum": TenantSupportPermission::ALL.map(TenantSupportPermission::as_str)
    })
}

fn positive_version_schema() -> Value {
    json!({
        "type": "integer",
        "format": "int64",
        "minimum": 1,
        "maximum": MAXIMUM_JSON_SAFE_INTEGER
    })
}

fn non_negative_version_schema() -> Value {
    json!({
        "type": "integer",
        "format": "int64",
        "minimum": 0,
        "maximum": MAXIMUM_JSON_SAFE_INTEGER
    })
}

fn nullable_timestamp_schema() -> Value {
    json!({ "allOf": [timestamp_schema()], "nullable": true })
}

fn nullable_schema_ref(name: &str) -> Value {
    json!({ "allOf": [schema_ref(name)], "nullable": true })
}

fn platform_role_policy_acl_example() -> String {
    PlatformRolePolicyContract::baseline(
        InstallationId::from_uuid(example_uuid(1)),
        PlatformRolePolicyId::from_uuid(example_uuid(2)),
    )
    .expect("fixed OpenAPI platform role policy example")
    .canonical_acl()
    .into()
}

fn tenant_support_grant_acl_example() -> String {
    let installation_id = InstallationId::from_uuid(example_uuid(1));
    let starts_at = Utc
        .with_ymd_and_hms(2026, 8, 29, 8, 0, 0)
        .single()
        .expect("fixed OpenAPI tenant support timestamp");
    TenantSupportGrantContract::from_spec(TenantSupportGrantContractSpec {
        grant_id: TenantSupportGrantId::from_uuid(example_uuid(3)),
        principal_id: PrincipalId::from_uuid(example_uuid(4)),
        scope: ScopeContext::organization(
            installation_id,
            OrganizationId::from_uuid(example_uuid(5)),
        )
        .expect("fixed OpenAPI tenant support scope"),
        permissions: vec![TenantSupportPermission::HealthRead],
        case_reference: "support-case-2026-0001".into(),
        justification_digest: Sha256Digest::parse(format!("sha256:{}", "a".repeat(64)))
            .expect("fixed OpenAPI tenant support digest"),
        mode: TenantSupportGrantMode::Standard,
        approval_requirement: TenantSupportApprovalRequirement::Single,
        approver_ids: vec![PrincipalId::from_uuid(example_uuid(6))],
        tenant_notification: TenantNotificationRequirement::Required,
        security_alert_required: true,
        post_incident_review_required: false,
        starts_at,
        expires_at: starts_at + Duration::minutes(30),
    })
    .expect("fixed OpenAPI tenant support grant example")
    .canonical_acl()
    .into()
}

fn trust_domain_acl_example() -> String {
    example_trust_domain_contract().canonical_acl().into()
}

fn example_trust_domain_contract() -> TrustDomainContract {
    TrustDomainContract::from_spec(TrustDomainContractSpec {
        installation_id: InstallationId::from_uuid(example_uuid(1)),
        trust_domain_id: TrustDomainId::from_uuid(example_uuid(7)),
        name: TrustDomainName::parse("prod.a3s.internal").expect("fixed OpenAPI trust-domain name"),
        provider_profile_digest: example_digest('b'),
        trust_bundle_digest: example_digest('c'),
        node_attestation_profile_digests: vec![example_digest('d')],
        identity_formats: vec![WorkloadIdentityFormat::X509Svid],
        max_credential_lifetime_seconds: 600,
        rotation_overlap_seconds: 60,
        revocation_mode: WorkloadIdentityRevocationMode::EpochAndExpiry,
        federation_bundle_digests: vec![],
    })
    .expect("fixed OpenAPI trust-domain example")
}

fn workload_identity_policy_acl_example() -> String {
    WorkloadIdentityPolicyContract::from_spec(WorkloadIdentityPolicySpec {
        installation_id: InstallationId::from_uuid(example_uuid(1)),
        trust_domain_id: TrustDomainId::from_uuid(example_uuid(7)),
        trust_domain_revision_id: TrustDomainRevisionId::from_uuid(example_uuid(8)),
        organization_id: OrganizationId::from_uuid(example_uuid(9)),
        project_id: ProjectId::from_uuid(example_uuid(10)),
        environment_id: EnvironmentId::from_uuid(example_uuid(11)),
        policy_id: WorkloadIdentityPolicyId::from_uuid(example_uuid(12)),
        workload_id: WorkloadId::from_uuid(example_uuid(13)),
        workload_revision_id: WorkloadRevisionId::from_uuid(example_uuid(14)),
        product_role: WorkloadProductRole::AgentService,
        runtime_class: RuntimeUnitClass::Service,
        semantics_profile_digest: example_digest('e'),
        node_pool_id: NodePoolId::from_uuid(example_uuid(15)),
        isolation_level: RuntimeIsolationLevel::Container,
        attestation_profile_digest: example_digest('d'),
        confidential_compute: false,
        identity_formats: vec![WorkloadIdentityFormat::X509Svid],
        credential_lifetime_seconds: 300,
        rotate_before_expiry_seconds: 60,
        drain_on_rotation_failure: true,
        revoke_on_stop: true,
        audiences: vec![WorkloadIdentityAudience::parse("model.internal")
            .expect("fixed OpenAPI workload identity audience")],
        service_names: vec![
            crate::modules::identity::domain::value_objects::PrivateServiceName::parse(
                "agent.prod.a3s.internal",
            )
            .expect("fixed OpenAPI private service name"),
        ],
        peer_policy_revision_digests: vec![],
    })
    .expect("fixed OpenAPI workload identity policy example")
    .canonical_acl()
    .into()
}

fn example_digest(value: char) -> Sha256Digest {
    Sha256Digest::parse(format!("sha256:{}", value.to_string().repeat(64)))
        .expect("fixed OpenAPI digest")
}

fn example_uuid(value: u128) -> Uuid {
    Uuid::from_u128(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn privileged_management_components_are_closed_bounded_and_domain_generated() {
        assert_eq!(PRIVILEGED_MANAGEMENT_SUCCESS_SCHEMA_BINDINGS.len(), 14);
        assert_eq!(PRIVILEGED_MANAGEMENT_SUCCESS_RESPONSE_BINDINGS.len(), 14);
        let mut schemas = Map::new();
        install_privileged_management_component_schemas(&mut schemas);
        assert_eq!(schemas.len(), 21);
        for (name, schema) in &schemas {
            assert_eq!(schema["additionalProperties"], false, "{name}");
        }
        assert_eq!(
            schemas["PlatformRolePolicy"]["properties"]["rolePermissions"]["maxItems"],
            PlatformRole::ALL.len()
        );
        assert_eq!(
            schemas["TenantSupportGrantProposal"]["properties"]["canonicalAcl"]["maxLength"],
            TENANT_SUPPORT_GRANT_MAX_ACL_BYTES
        );
        PlatformRolePolicyContract::parse_acl(&platform_role_policy_acl_example())
            .expect("policy example remains a valid domain contract");
        TenantSupportGrantContract::parse_acl(&tenant_support_grant_acl_example())
            .expect("support example remains a valid domain contract");
        TrustDomainContract::parse_acl(&trust_domain_acl_example())
            .expect("trust-domain example remains a valid domain contract");
        WorkloadIdentityPolicyContract::parse_acl(&workload_identity_policy_acl_example())
            .expect("workload identity policy example remains a valid domain contract");

        for request in [
            accept_platform_role_policy_request_schema(),
            create_platform_role_binding_request_schema(),
            change_platform_role_binding_request_schema(),
            expected_version_request_schema(),
            propose_tenant_support_grant_request_schema(),
            approve_tenant_support_grant_request_schema(),
            accept_trust_domain_revision_request_schema(),
            accept_workload_identity_policy_revision_request_schema(),
        ] {
            assert_eq!(request["additionalProperties"], false);
            let encoded = request.to_string();
            for forbidden in ["actorPrincipalId", "credentialId", "installationId"] {
                assert!(!encoded.contains(forbidden), "request exposes {forbidden}");
            }
        }
    }
}
