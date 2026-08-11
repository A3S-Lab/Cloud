use crate::modules::assets::domain::IMcpServiceProfileRepository;
use crate::modules::edge::domain::events::McpRoutePolicyMutationKind;
use crate::modules::edge::domain::repositories::{
    IMcpRoutePolicyRepository, McpRoutePolicyWrite, MutateMcpRoutePolicyWrite,
};
use crate::modules::edge::domain::{McpRoutePolicy, McpRoutePolicyDocument};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, IdempotencyRequest, OrganizationId, ProjectId,
    RepositoryError, RouteId,
};
use chrono::{DateTime, Utc};
use std::sync::Arc;
use uuid::Uuid;

pub struct McpRoutePolicyApplicationService {
    policies: Arc<dyn IMcpRoutePolicyRepository>,
    profiles: Arc<dyn IMcpServiceProfileRepository>,
}

impl McpRoutePolicyApplicationService {
    pub fn new(
        policies: Arc<dyn IMcpRoutePolicyRepository>,
        profiles: Arc<dyn IMcpServiceProfileRepository>,
    ) -> Self {
        Self { policies, profiles }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        acl: String,
        idempotency_key: String,
        request_id: Uuid,
        requested_at: DateTime<Utc>,
    ) -> ApplicationResult<McpRoutePolicyWrite> {
        let requested_at = normalize_request(request_id, requested_at)?;
        let document = McpRoutePolicy::parse_acl(&acl).map_err(ApplicationError::Invalid)?;
        let spec = document.spec();
        if spec.organization_id != organization_id
            || spec.project_id != project_id
            || spec.environment_id != environment_id
            || document.policy_revision() != 1
        {
            return Err(ApplicationError::Invalid(
                "MCP route policy ACL does not match its create scope or initial revision".into(),
            ));
        }
        let profile = self.profile(&document).await?;
        let preflight = document
            .materialize(requested_at, requested_at, &profile)
            .map(|_| ())
            .map_err(ApplicationError::Invalid);
        let idempotency = idempotency(
            format!(
                "organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/mcp-route-policies"
            ),
            idempotency_key,
            &document,
        )?;
        self.mutate(
            MutateMcpRoutePolicyWrite {
                document,
                kind: McpRoutePolicyMutationKind::Create,
                idempotency,
                request_id,
                requested_at: canonical_timestamp(requested_at),
            },
            preflight.err(),
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn revise(
        &self,
        organization_id: OrganizationId,
        route_id: RouteId,
        acl: String,
        idempotency_key: String,
        request_id: Uuid,
        requested_at: DateTime<Utc>,
    ) -> ApplicationResult<McpRoutePolicyWrite> {
        let requested_at = normalize_request(request_id, requested_at)?;
        let document = McpRoutePolicy::parse_acl(&acl).map_err(ApplicationError::Invalid)?;
        if document.spec().organization_id != organization_id
            || document.spec().route_id != route_id
        {
            return Err(ApplicationError::Invalid(
                "MCP route policy ACL does not match its revision path".into(),
            ));
        }
        let current = self.get(organization_id, route_id).await?;
        let profile = self.profile(&document).await?;
        let preflight = validate_revision(&current, &document, &profile, requested_at);
        let idempotency = idempotency(
            format!("organizations/{organization_id}/mcp-route-policies/{route_id}/revisions"),
            idempotency_key,
            &document,
        )?;
        self.mutate(
            MutateMcpRoutePolicyWrite {
                document,
                kind: McpRoutePolicyMutationKind::Revise,
                idempotency,
                request_id,
                requested_at: canonical_timestamp(requested_at),
            },
            preflight.err(),
        )
        .await
    }

    pub async fn get(
        &self,
        organization_id: OrganizationId,
        route_id: RouteId,
    ) -> ApplicationResult<McpRoutePolicy> {
        self.policies
            .find_mcp_route_policy(organization_id, route_id)
            .await
            .map_err(ApplicationError::from)?
            .ok_or_else(|| ApplicationError::NotFound("MCP route policy not found".into()))
    }

    pub async fn list(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> ApplicationResult<Vec<McpRoutePolicy>> {
        self.policies
            .list_mcp_route_policies(organization_id, project_id, environment_id)
            .await
            .map_err(ApplicationError::from)
    }

    async fn profile(
        &self,
        document: &McpRoutePolicyDocument,
    ) -> ApplicationResult<crate::modules::assets::domain::McpServiceProfile> {
        let spec = document.spec();
        let binding = self
            .profiles
            .find_mcp_service_profile(spec.organization_id, spec.asset_id, spec.asset_release_id)
            .await
            .map_err(ApplicationError::from)?
            .ok_or_else(|| {
                ApplicationError::NotFound("MCP Service profile binding not found".into())
            })?;
        Ok(binding.profile)
    }

    async fn mutate(
        &self,
        write: MutateMcpRoutePolicyWrite,
        preflight_error: Option<ApplicationError>,
    ) -> ApplicationResult<McpRoutePolicyWrite> {
        match self.policies.mutate_mcp_route_policy(write).await {
            Ok(value) => Ok(value),
            Err(error) => match preflight_error {
                Some(preflight)
                    if matches!(
                        &error,
                        RepositoryError::NotFound | RepositoryError::Conflict(_)
                    ) =>
                {
                    Err(preflight)
                }
                _ => Err(error.into()),
            },
        }
    }
}

fn normalize_request(
    request_id: Uuid,
    requested_at: DateTime<Utc>,
) -> ApplicationResult<DateTime<Utc>> {
    if request_id.is_nil() {
        return Err(ApplicationError::Invalid(
            "MCP route policy request identity is invalid".into(),
        ));
    }
    Ok(canonical_timestamp(requested_at))
}

fn idempotency(
    scope: String,
    key: String,
    document: &McpRoutePolicyDocument,
) -> ApplicationResult<IdempotencyRequest> {
    IdempotencyRequest::new(scope, key, document.canonical_acl().as_bytes())
        .map_err(ApplicationError::Invalid)
}

fn validate_revision(
    current: &McpRoutePolicy,
    document: &McpRoutePolicyDocument,
    profile: &crate::modules::assets::domain::McpServiceProfile,
    requested_at: DateTime<Utc>,
) -> ApplicationResult<()> {
    if current.spec() == document.spec()
        && current.policy_revision() == document.policy_revision()
        && current.policy_digest() == document.policy_digest()
    {
        return Ok(());
    }
    let expected_revision = current.policy_revision().checked_add(1).ok_or_else(|| {
        ApplicationError::Conflict("MCP route policy revision is exhausted".into())
    })?;
    if document.policy_revision() != expected_revision {
        return Err(ApplicationError::Conflict(
            "MCP route policy revision does not follow the current revision".into(),
        ));
    }
    let mut revised = current.clone();
    let changed = revised
        .revise(document.spec().clone(), profile, requested_at)
        .map_err(ApplicationError::Invalid)?;
    if !changed {
        return Err(ApplicationError::Invalid(
            "MCP route policy revision does not change desired state".into(),
        ));
    }
    if revised.canonical_acl() != document.canonical_acl()
        || revised.policy_digest() != document.policy_digest()
        || revised.policy_revision() != document.policy_revision()
    {
        return Err(ApplicationError::Invalid(
            "MCP route policy revision is not canonical".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Timelike};

    #[test]
    fn normalizes_request_time_before_repository_admission() {
        let requested_at = Utc
            .timestamp_opt(1_700_000_000, 123_456_789)
            .single()
            .expect("timestamp");

        let normalized = normalize_request(Uuid::now_v7(), requested_at).expect("request");

        assert_eq!(normalized.nanosecond(), 123_456_000);
        assert!(normalize_request(Uuid::nil(), requested_at).is_err());
    }
}
