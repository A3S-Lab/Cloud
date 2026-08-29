use super::ResourceAccessEvaluator;
use crate::modules::identity::domain::entities::{ApiToken, Membership, ResourceGrant};
use crate::modules::identity::domain::value_objects::{
    ApiTokenScope, MembershipRole, ResourceGrantScope,
};
use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, canonical_timestamp, sha256_digest, validate_audit_action, ApiTokenId,
    AuthorizationDecisionRef, MembershipId, OrganizationId, PrincipalId, ResourceGrantId,
    Sha256Digest,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const RESOURCE_AUTHORIZATION_DECISION_API_VERSION: &str =
    "a3s.dev/cloud/resource-authorization-decision/v1";
const RESOURCE_AUTHORIZATION_DECISION_REFERENCE_PREFIX: &str =
    "urn:a3s:cloud:identity:resource-authorization-decision:";
const RESOURCE_AUTHORIZATION_DECISION_MAX_BYTES: usize = 128 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResourceAuthorizationDecisionRequest {
    pub organization_id: OrganizationId,
    pub principal_id: PrincipalId,
    pub credential_id: ApiTokenId,
    pub required_scope: ApiTokenScope,
    pub action: String,
    pub resource: ResourceGrantScope,
    pub request_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResourceAuthorizationGrantEvidence {
    pub id: ResourceGrantId,
    pub aggregate_version: u64,
    pub scope: ResourceGrantScope,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResourceAuthorizationCredentialEvidence {
    pub id: ApiTokenId,
    pub aggregate_version: u64,
    pub scopes: Vec<ApiTokenScope>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ResourceAuthorizationBasis {
    Membership {
        membership_id: MembershipId,
        membership_version: u64,
        role: MembershipRole,
        grants: Vec<ResourceAuthorizationGrantEvidence>,
    },
}

/// Immutable evidence emitted by Identity after resolving current Membership and Resource Grant
/// authority through the shared [`ResourceAccessEvaluator`]. Workflow receives only the resulting
/// reference and therefore cannot manufacture an authorization fact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResourceAuthorizationDecision {
    pub api_version: String,
    pub id: Uuid,
    pub organization_id: OrganizationId,
    pub principal_id: PrincipalId,
    pub credential: ResourceAuthorizationCredentialEvidence,
    pub required_scope: ApiTokenScope,
    pub action: String,
    pub resource: ResourceGrantScope,
    pub basis: ResourceAuthorizationBasis,
    pub request_id: Uuid,
    pub decided_at: DateTime<Utc>,
    pub digest: Sha256Digest,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ResourceAuthorizationDecisionDigestContent<'a> {
    api_version: &'a str,
    id: Uuid,
    organization_id: OrganizationId,
    principal_id: PrincipalId,
    credential: &'a ResourceAuthorizationCredentialEvidence,
    required_scope: &'a ApiTokenScope,
    action: &'a str,
    resource: ResourceGrantScope,
    basis: &'a ResourceAuthorizationBasis,
    request_id: Uuid,
    decided_at: DateTime<Utc>,
}

impl ResourceAuthorizationDecisionRequest {
    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.principal_id.as_uuid().is_nil()
            || self.credential_id.as_uuid().is_nil()
            || self.request_id.is_nil()
            || validate_audit_action(&self.action).is_err()
        {
            return Err("resource authorization decision request is invalid".into());
        }
        Ok(())
    }
}

impl ResourceAuthorizationDecision {
    pub fn issue_membership(
        id: Uuid,
        request: ResourceAuthorizationDecisionRequest,
        credential: &ApiToken,
        membership: &Membership,
        grants: impl IntoIterator<Item = ResourceGrant>,
        decided_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        request.validate()?;
        if !membership.is_active()
            || membership.organization_id != request.organization_id
            || membership.principal_id != request.principal_id
        {
            return Err("membership authorization evidence does not match the request".into());
        }
        let mut grants = grants.into_iter().collect::<Vec<_>>();
        grants.sort_by_key(|grant| grant.id);
        if grants.iter().any(|grant| {
            !grant.is_active()
                || grant.organization_id != membership.organization_id
                || grant.membership_id != membership.id
        }) {
            return Err("Resource Grant evidence does not match the membership".into());
        }
        if membership.role != MembershipRole::Restricted && !grants.is_empty() {
            return Err(
                "organization-wide membership evidence must not carry Resource Grants".into(),
            );
        }
        let basis = ResourceAuthorizationBasis::Membership {
            membership_id: membership.id,
            membership_version: membership.aggregate_version,
            role: membership.role,
            grants: grants
                .into_iter()
                .map(|grant| ResourceAuthorizationGrantEvidence {
                    id: grant.id,
                    aggregate_version: grant.aggregate_version,
                    scope: grant.scope,
                })
                .collect(),
        };
        Self::issue(id, request, credential, basis, decided_at)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.api_version != RESOURCE_AUTHORIZATION_DECISION_API_VERSION
            || self.id.is_nil()
            || self.organization_id.as_uuid().is_nil()
            || self.principal_id.as_uuid().is_nil()
            || self.credential.id.as_uuid().is_nil()
            || self.credential.aggregate_version == 0
            || self.credential.scopes.is_empty()
            || self
                .credential
                .scopes
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
            || !self.credential.scopes.contains(&self.required_scope)
            || self.request_id.is_nil()
            || self.decided_at != canonical_timestamp(self.decided_at)
            || validate_audit_action(&self.action).is_err()
            || !basis_evaluator(&self.basis)?.allows(self.resource)
            || self.compute_digest()? != self.digest
        {
            return Err("resource authorization decision is invalid".into());
        }
        Ok(())
    }

    pub fn reference(&self) -> Result<AuthorizationDecisionRef, String> {
        self.validate()?;
        AuthorizationDecisionRef::new(
            format!(
                "{RESOURCE_AUTHORIZATION_DECISION_REFERENCE_PREFIX}{}",
                self.id
            ),
            self.digest.clone(),
        )
    }

    pub const fn audit_action() -> &'static str {
        "identity.resource-access.authorize"
    }

    pub const fn aggregate_id(&self) -> Uuid {
        match self.resource {
            ResourceGrantScope::Project { project_id } => project_id.as_uuid(),
            ResourceGrantScope::Environment { environment_id, .. } => environment_id.as_uuid(),
            ResourceGrantScope::Node { node_id } => node_id.as_uuid(),
        }
    }

    fn issue(
        id: Uuid,
        request: ResourceAuthorizationDecisionRequest,
        credential: &ApiToken,
        basis: ResourceAuthorizationBasis,
        decided_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        request.validate()?;
        let decided_at = canonical_timestamp(decided_at);
        if credential.id != request.credential_id
            || credential.organization_id != request.organization_id
            || credential.principal_id != request.principal_id
            || decided_at < credential.created_at
            || !credential.is_active_at(decided_at)
            || !credential.scopes.contains(&request.required_scope)
        {
            return Err("authorization credential evidence does not match the request".into());
        }
        if !basis_evaluator(&basis)?.allows(request.resource) {
            return Err("resource authorization decision was not allowed".into());
        }
        let mut value = Self {
            api_version: RESOURCE_AUTHORIZATION_DECISION_API_VERSION.into(),
            id,
            organization_id: request.organization_id,
            principal_id: request.principal_id,
            credential: ResourceAuthorizationCredentialEvidence {
                id: credential.id,
                aggregate_version: credential.aggregate_version,
                scopes: credential.scopes.iter().cloned().collect(),
            },
            required_scope: request.required_scope,
            action: request.action,
            resource: request.resource,
            basis,
            request_id: request.request_id,
            decided_at,
            digest: Sha256Digest::parse(format!("sha256:{}", "0".repeat(64)))?,
        };
        value.digest = value.compute_digest()?;
        value.validate()?;
        Ok(value)
    }

    fn compute_digest(&self) -> Result<Sha256Digest, String> {
        let content = ResourceAuthorizationDecisionDigestContent {
            api_version: &self.api_version,
            id: self.id,
            organization_id: self.organization_id,
            principal_id: self.principal_id,
            credential: &self.credential,
            required_scope: &self.required_scope,
            action: &self.action,
            resource: self.resource,
            basis: &self.basis,
            request_id: self.request_id,
            decided_at: self.decided_at,
        };
        let canonical = canonical_json_bounded(
            &content,
            RESOURCE_AUTHORIZATION_DECISION_MAX_BYTES,
            "Resource authorization decision digest content",
        )?;
        Sha256Digest::parse(sha256_digest(&canonical))
    }
}

fn basis_evaluator(basis: &ResourceAuthorizationBasis) -> Result<ResourceAccessEvaluator, String> {
    match basis {
        ResourceAuthorizationBasis::Membership {
            membership_id,
            membership_version,
            role,
            grants,
        } => {
            if membership_id.as_uuid().is_nil()
                || *membership_version == 0
                || grants
                    .iter()
                    .any(|grant| grant.id.as_uuid().is_nil() || grant.aggregate_version == 0)
                || grants.windows(2).any(|pair| pair[0].id >= pair[1].id)
                || (*role != MembershipRole::Restricted && !grants.is_empty())
            {
                return Err("resource authorization basis is invalid".into());
            }
            Ok(ResourceAccessEvaluator::for_membership(
                *role,
                grants.iter().map(|grant| grant.scope),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::identity::domain::value_objects::ApiTokenName;
    use crate::modules::shared_kernel::domain::{ApiTokenId, EnvironmentId, ProjectId};
    use chrono::TimeZone;
    use std::collections::BTreeSet;

    fn timestamp() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 12, 12, 0, 0)
            .single()
            .expect("timestamp")
    }

    fn request(
        organization_id: OrganizationId,
        principal_id: PrincipalId,
        project_id: ProjectId,
    ) -> ResourceAuthorizationDecisionRequest {
        ResourceAuthorizationDecisionRequest {
            organization_id,
            principal_id,
            credential_id: ApiTokenId::new(),
            required_scope: ApiTokenScope::parse(ApiTokenScope::WORKFLOW_WRITE).expect("scope"),
            action: "workflow.human-task.submit".into(),
            resource: ResourceGrantScope::Project { project_id },
            request_id: Uuid::now_v7(),
        }
    }

    fn credential(request: &ResourceAuthorizationDecisionRequest) -> ApiToken {
        ApiToken::issue(
            request.credential_id,
            request.organization_id,
            request.principal_id,
            ApiTokenName::parse("human task reviewer").expect("name"),
            BTreeSet::from([request.required_scope.clone()]),
            timestamp(),
            None,
        )
        .expect("credential")
    }

    #[test]
    fn binds_membership_and_grant_versions_into_a_stable_reference() {
        let organization_id = OrganizationId::new();
        let principal_id = PrincipalId::new();
        let project_id = ProjectId::new();
        let membership = Membership::create(
            MembershipId::new(),
            organization_id,
            principal_id,
            MembershipRole::Restricted,
            timestamp(),
        );
        let grant = ResourceGrant::create(
            ResourceGrantId::new(),
            organization_id,
            membership.id,
            ResourceGrantScope::Project { project_id },
            timestamp(),
        );
        let request = request(organization_id, principal_id, project_id);
        let decision = ResourceAuthorizationDecision::issue_membership(
            Uuid::now_v7(),
            request.clone(),
            &credential(&request),
            &membership,
            [grant],
            timestamp(),
        )
        .expect("decision");

        decision.validate().expect("valid decision");
        let reference = decision.reference().expect("reference");
        assert!(reference.id.ends_with(&decision.id.to_string()));
        assert_eq!(reference.digest, decision.digest);
    }

    #[test]
    fn refuses_a_membership_without_project_authority() {
        let organization_id = OrganizationId::new();
        let principal_id = PrincipalId::new();
        let project_id = ProjectId::new();
        let membership = Membership::create(
            MembershipId::new(),
            organization_id,
            principal_id,
            MembershipRole::Restricted,
            timestamp(),
        );
        let grant = ResourceGrant::create(
            ResourceGrantId::new(),
            organization_id,
            membership.id,
            ResourceGrantScope::Environment {
                project_id,
                environment_id: EnvironmentId::new(),
            },
            timestamp(),
        );
        let request = request(organization_id, principal_id, project_id);
        let error = ResourceAuthorizationDecision::issue_membership(
            Uuid::now_v7(),
            request.clone(),
            &credential(&request),
            &membership,
            [grant],
            timestamp(),
        )
        .expect_err("environment authority cannot authorize a project action");

        assert_eq!(error, "resource authorization decision was not allowed");
    }

    #[test]
    fn platform_scope_does_not_expand_restricted_membership_authority() {
        let organization_id = OrganizationId::new();
        let principal_id = PrincipalId::new();
        let project_id = ProjectId::new();
        let membership = Membership::create(
            MembershipId::new(),
            organization_id,
            principal_id,
            MembershipRole::Restricted,
            timestamp(),
        );
        let request = request(organization_id, principal_id, project_id);
        let credential = ApiToken::issue(
            request.credential_id,
            organization_id,
            principal_id,
            ApiTokenName::parse("restricted platform operator").expect("name"),
            BTreeSet::from([
                request.required_scope.clone(),
                ApiTokenScope::parse(ApiTokenScope::PLATFORM_WRITE).expect("platform scope"),
            ]),
            timestamp(),
            None,
        )
        .expect("credential");

        let error = ResourceAuthorizationDecision::issue_membership(
            Uuid::now_v7(),
            request,
            &credential,
            &membership,
            [],
            timestamp(),
        )
        .expect_err("platform scope cannot replace a Resource Grant");

        assert_eq!(error, "resource authorization decision was not allowed");
    }

    #[test]
    fn digest_detects_membership_evidence_drift() {
        let organization_id = OrganizationId::new();
        let principal_id = PrincipalId::new();
        let project_id = ProjectId::new();
        let membership = Membership::create(
            MembershipId::new(),
            organization_id,
            principal_id,
            MembershipRole::Member,
            timestamp(),
        );
        let request = request(organization_id, principal_id, project_id);
        let mut decision = ResourceAuthorizationDecision::issue_membership(
            Uuid::now_v7(),
            request.clone(),
            &credential(&request),
            &membership,
            [],
            timestamp(),
        )
        .expect("decision");
        let ResourceAuthorizationBasis::Membership {
            membership_version, ..
        } = &mut decision.basis;
        *membership_version += 1;

        assert!(decision.validate().is_err());
    }
}
