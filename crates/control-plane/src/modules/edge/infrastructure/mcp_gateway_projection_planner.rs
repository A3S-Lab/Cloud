use crate::modules::edge::domain::repositories::IMcpCredentialRepository;
use crate::modules::edge::infrastructure::{McpRouteProjectionPlanner, PlanMcpRouteProjection};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, McpCredentialId, NodeId, RepositoryError,
};
use a3s_cloud_contracts::{McpGatewayProjection, MCP_GATEWAY_PROJECTION_SCHEMA};
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::Arc;

const MAX_SAFE_ACL_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct McpCredentialProjectionVersion {
    credential_id: McpCredentialId,
    generation: u64,
    aggregate_version: u64,
}

impl McpCredentialProjectionVersion {
    pub fn new(
        credential_id: McpCredentialId,
        generation: u64,
        aggregate_version: u64,
    ) -> Result<Self, String> {
        if credential_id.as_uuid().is_nil()
            || generation == 0
            || generation > MAX_SAFE_ACL_INTEGER
            || aggregate_version < generation
            || aggregate_version > MAX_SAFE_ACL_INTEGER
        {
            return Err("MCP credential projection version is invalid".into());
        }
        Ok(Self {
            credential_id,
            generation,
            aggregate_version,
        })
    }

    pub const fn credential_id(self) -> McpCredentialId {
        self.credential_id
    }

    pub const fn generation(self) -> u64 {
        self.generation
    }

    pub const fn aggregate_version(self) -> u64 {
        self.aggregate_version
    }
}

/// Exact credential-authority evidence observed while reconciling an MCP
/// route. Unlike projection versions, this also records credentials whose
/// current generation or lifecycle state makes the route ineligible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct McpCredentialAuthorityVersion {
    credential_id: McpCredentialId,
    generation: u64,
    aggregate_version: u64,
    active_at_observed_at: bool,
}

impl McpCredentialAuthorityVersion {
    pub fn new(
        credential_id: McpCredentialId,
        generation: u64,
        aggregate_version: u64,
        active_at_observed_at: bool,
    ) -> Result<Self, String> {
        McpCredentialProjectionVersion::new(credential_id, generation, aggregate_version)?;
        Ok(Self {
            credential_id,
            generation,
            aggregate_version,
            active_at_observed_at,
        })
    }

    pub const fn credential_id(self) -> McpCredentialId {
        self.credential_id
    }

    pub const fn generation(self) -> u64 {
        self.generation
    }

    pub const fn aggregate_version(self) -> u64 {
        self.aggregate_version
    }

    pub const fn active_at_observed_at(self) -> bool {
        self.active_at_observed_at
    }
}

/// A complete MCP snapshot bound to the one physical Gateway that may receive
/// its node-local Runtime endpoints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedMcpGatewayProjection {
    gateway_node_id: NodeId,
    projection: McpGatewayProjection,
    credential_versions: Vec<McpCredentialProjectionVersion>,
}

impl PlannedMcpGatewayProjection {
    pub(crate) fn new(
        gateway_node_id: NodeId,
        projection: McpGatewayProjection,
        mut credential_versions: Vec<McpCredentialProjectionVersion>,
        observed_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        if gateway_node_id.as_uuid().is_nil() {
            return Err("MCP Gateway projection target node must not be nil".into());
        }
        projection.validate(canonical_timestamp(observed_at))?;
        if projection
            .routes
            .iter()
            .flat_map(|route| &route.targets)
            .any(|target| target.node_id != gateway_node_id.as_uuid())
        {
            return Err(
                "MCP Gateway projection contains a Runtime target from another physical node"
                    .into(),
            );
        }
        credential_versions.sort_by_key(|version| version.credential_id);
        if credential_versions.len() != projection.credentials.len()
            || credential_versions
                .windows(2)
                .any(|versions| versions[0].credential_id == versions[1].credential_id)
            || credential_versions.iter().any(|version| {
                projection.credentials.iter().all(|credential| {
                    credential.credential_id != version.credential_id.as_uuid()
                        || credential.generation != version.generation
                })
            })
        {
            return Err(
                "MCP Gateway projection credential version vector is incomplete or inconsistent"
                    .into(),
            );
        }
        Ok(Self {
            gateway_node_id,
            projection,
            credential_versions,
        })
    }

    pub const fn gateway_node_id(&self) -> NodeId {
        self.gateway_node_id
    }

    pub const fn projection(&self) -> &McpGatewayProjection {
        &self.projection
    }

    pub fn credential_versions(&self) -> &[McpCredentialProjectionVersion] {
        &self.credential_versions
    }

    #[cfg(test)]
    pub(crate) fn into_projection(self) -> McpGatewayProjection {
        self.projection
    }

    pub(crate) fn into_parts(
        self,
    ) -> (
        NodeId,
        McpGatewayProjection,
        Vec<McpCredentialProjectionVersion>,
    ) {
        (
            self.gateway_node_id,
            self.projection,
            self.credential_versions,
        )
    }
}

/// Resolves route grants against Cloud's credential authority and assembles a
/// complete node-local MCP projection for one hosted route.
///
/// Multi-route snapshot aggregation and managed publication remain outside
/// this planner so replacing one route cannot silently remove another.
#[derive(Clone)]
pub struct McpGatewayProjectionPlanner {
    routes: McpRouteProjectionPlanner,
    credentials: Arc<dyn IMcpCredentialRepository>,
}

pub(crate) struct ReconciledMcpGatewayProjection {
    projection: Option<PlannedMcpGatewayProjection>,
    credential_authority_versions: Vec<McpCredentialAuthorityVersion>,
}

impl ReconciledMcpGatewayProjection {
    pub(crate) fn into_parts(
        self,
    ) -> (
        Option<PlannedMcpGatewayProjection>,
        Vec<McpCredentialAuthorityVersion>,
    ) {
        (self.projection, self.credential_authority_versions)
    }
}

impl McpGatewayProjectionPlanner {
    pub fn new(
        routes: McpRouteProjectionPlanner,
        credentials: Arc<dyn IMcpCredentialRepository>,
    ) -> Self {
        Self {
            routes,
            credentials,
        }
    }

    pub async fn plan(
        &self,
        request: PlanMcpRouteProjection,
    ) -> Result<PlannedMcpGatewayProjection, RepositoryError> {
        self.plan_for_reconciliation(request)
            .await?
            .projection
            .ok_or_else(|| {
                RepositoryError::Conflict(
                    "MCP route grant credential generation or state is invalid".into(),
                )
            })
    }

    pub(crate) async fn plan_for_reconciliation(
        &self,
        request: PlanMcpRouteProjection,
    ) -> Result<ReconciledMcpGatewayProjection, RepositoryError> {
        let observed_at = McpRouteProjectionPlanner::validate_request(&request)?;
        let spec = request.policy.spec();
        let organization_id = spec.organization_id;
        let project_id = spec.project_id;
        let environment_id = spec.environment_id;
        let policy_expires_at = spec.expires_at;
        let grants = spec.grants.clone();
        let gateway_node_id = request.gateway_node_id;
        let profile = request.profile_binding.profile.gateway_projection();

        let credential_ids = grants
            .iter()
            .map(|grant| McpCredentialId::from_uuid(grant.credential_id))
            .collect::<Vec<_>>();
        let available = self
            .credentials
            .resolve_mcp_credentials(organization_id, project_id, environment_id, &credential_ids)
            .await?;
        let mut by_id = HashMap::with_capacity(available.len());
        for credential in available {
            if by_id.insert(credential.id, credential).is_some() {
                return Err(RepositoryError::Storage(
                    "MCP credential authority returned a duplicate identity".into(),
                ));
            }
        }

        let mut credentials = Vec::with_capacity(grants.len());
        let mut credential_versions = Vec::with_capacity(grants.len());
        let mut credential_authority_versions = Vec::with_capacity(grants.len());
        let mut expires_at = policy_expires_at;
        let mut eligible = true;
        for grant in &grants {
            let credential_id = McpCredentialId::from_uuid(grant.credential_id);
            let credential = by_id.remove(&credential_id).ok_or_else(|| {
                RepositoryError::Conflict(
                    "MCP route grant references an unavailable environment credential".into(),
                )
            })?;
            if credential.organization_id != organization_id
                || credential.project_id != project_id
                || credential.environment_id != environment_id
            {
                return Err(RepositoryError::Conflict(
                    "MCP route grant credential scope is invalid".into(),
                ));
            }
            let active_at_observed_at = credential.is_active_at(observed_at);
            credential_authority_versions.push(
                McpCredentialAuthorityVersion::new(
                    credential.id,
                    credential.generation(),
                    credential.aggregate_version(),
                    active_at_observed_at,
                )
                .map_err(RepositoryError::Storage)?,
            );
            eligible &=
                credential.generation() == grant.credential_generation && active_at_observed_at;
            expires_at = expires_at.min(credential.expires_at());
            credential_versions.push(
                McpCredentialProjectionVersion::new(
                    credential.id,
                    credential.generation(),
                    credential.aggregate_version(),
                )
                .map_err(RepositoryError::Storage)?,
            );
            credentials.push(credential.gateway_projection());
        }
        credential_authority_versions.sort_by_key(|version| version.credential_id());
        if !eligible {
            return Ok(ReconciledMcpGatewayProjection {
                projection: None,
                credential_authority_versions,
            });
        }

        let route = self.routes.plan(request).await?;
        if route.grants != grants {
            return Err(RepositoryError::Storage(
                "MCP route projection changed its credential grants while planning".into(),
            ));
        }
        credentials.sort_by_key(|credential| credential.credential_id);

        let projection = McpGatewayProjection {
            schema: MCP_GATEWAY_PROJECTION_SCHEMA.into(),
            expires_at,
            profiles: vec![profile],
            credentials,
            routes: vec![route],
        };
        let projection = PlannedMcpGatewayProjection::new(
            gateway_node_id,
            projection,
            credential_versions,
            observed_at,
        )
        .map_err(RepositoryError::Conflict)?;
        Ok(ReconciledMcpGatewayProjection {
            projection: Some(projection),
            credential_authority_versions,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::assets::domain::McpServiceProfileBinding;
    use crate::modules::edge::domain::repositories::IMcpCredentialRepository;
    use crate::modules::edge::domain::services::{IRouteTargetReader, ResolvedRouteTarget};
    use crate::modules::edge::domain::{GatewayScope, McpCredential, RoutePortName};
    use crate::modules::edge::infrastructure::mcp_route_target_projection_compiler::tests::{
        fixture, now, target,
    };
    use crate::modules::edge::infrastructure::McpRouteTargetProjectionCompiler;
    use crate::modules::edge::InMemoryEdgeRepository;
    use crate::modules::shared_kernel::domain::{
        EnvironmentId, NodeId, OrganizationId, ProjectId, WorkloadRevisionId,
    };
    use async_trait::async_trait;
    use chrono::Duration;

    const VERIFIER: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQxMjM0NTY3OA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    const ROTATED_VERIFIER: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQxMjM0NTY3OA$BAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

    struct FixedTargetReader(ResolvedRouteTarget);

    #[async_trait]
    impl IRouteTargetReader for FixedTargetReader {
        async fn resolve_healthy_target(
            &self,
            _organization_id: OrganizationId,
            _project_id: ProjectId,
            _environment_id: EnvironmentId,
            _revision_id: WorkloadRevisionId,
            _port_name: &RoutePortName,
            _now: chrono::DateTime<chrono::Utc>,
        ) -> Result<ResolvedRouteTarget, RepositoryError> {
            Ok(self.0.clone())
        }
    }

    fn scope(
        fixture: &crate::modules::edge::infrastructure::mcp_route_target_projection_compiler::tests::Fixture,
        node_id: NodeId,
    ) -> GatewayScope {
        let policy = fixture.policy.spec();
        GatewayScope::create(
            policy.gateway_scope_id,
            policy.organization_id,
            policy.project_id,
            policy.environment_id,
            node_id,
            now(),
        )
        .expect("scope")
    }

    fn profile_binding(
        fixture: &crate::modules::edge::infrastructure::mcp_route_target_projection_compiler::tests::Fixture,
    ) -> McpServiceProfileBinding {
        let policy = fixture.policy.spec();
        McpServiceProfileBinding {
            organization_id: policy.organization_id,
            asset_id: policy.asset_id,
            asset_release_id: policy.asset_release_id,
            profile: fixture.profile.clone(),
            created_at: now(),
        }
    }

    fn credential(
        fixture: &crate::modules::edge::infrastructure::mcp_route_target_projection_compiler::tests::Fixture,
    ) -> McpCredential {
        let policy = fixture.policy.spec();
        McpCredential::issue(
            McpCredentialId::from_uuid(policy.grants[0].credential_id),
            policy.organization_id,
            policy.project_id,
            policy.environment_id,
            "a3s_mcp_abc12345def67890",
            VERIFIER,
            now() + Duration::minutes(30),
            now(),
        )
        .expect("credential")
    }

    fn request(
        fixture: &crate::modules::edge::infrastructure::mcp_route_target_projection_compiler::tests::Fixture,
        node_id: NodeId,
    ) -> PlanMcpRouteProjection {
        PlanMcpRouteProjection {
            policy: fixture.policy.clone(),
            profile_binding: profile_binding(fixture),
            revision: fixture.revision.clone(),
            scope: scope(fixture, node_id),
            gateway_node_id: node_id,
            observed_at: now(),
        }
    }

    fn planner(
        fixture: &crate::modules::edge::infrastructure::mcp_route_target_projection_compiler::tests::Fixture,
        node_id: NodeId,
        credentials: Arc<dyn IMcpCredentialRepository>,
    ) -> McpGatewayProjectionPlanner {
        let targets: Arc<dyn IRouteTargetReader> =
            Arc::new(FixedTargetReader(target(fixture, node_id, 49152)));
        let routes = McpRouteProjectionPlanner::new(targets, McpRouteTargetProjectionCompiler);
        McpGatewayProjectionPlanner::new(routes, credentials)
    }

    #[tokio::test]
    async fn resolves_exact_grants_and_bounds_projection_expiry() {
        let fixture = fixture();
        let node_id = NodeId::new();
        let credentials = Arc::new(InMemoryEdgeRepository::new());
        let credential = credentials
            .create_mcp_credential(credential(&fixture))
            .await
            .expect("store credential");
        let planner = planner(&fixture, node_id, credentials);

        let projection = planner
            .plan(request(&fixture, node_id))
            .await
            .expect("projection");
        assert_eq!(projection.gateway_node_id(), node_id);
        assert_eq!(
            projection.credential_versions(),
            &[McpCredentialProjectionVersion::new(
                credential.id,
                credential.generation(),
                credential.aggregate_version(),
            )
            .expect("credential version")]
        );
        let projection = projection.projection();

        assert_eq!(projection.expires_at, credential.expires_at());
        assert_eq!(projection.credentials.len(), 1);
        assert_eq!(
            projection.credentials[0].credential_id,
            credential.id.as_uuid()
        );
        assert_eq!(projection.credentials[0].generation, 1);
        assert_eq!(projection.routes.len(), 1);
        assert_eq!(projection.routes[0].targets.len(), 1);
        projection.validate(now()).expect("valid projection");
        assert!(!format!("{projection:?}").contains(VERIFIER));
    }

    #[tokio::test]
    async fn node_binding_rejects_a_projection_with_a_remote_loopback_target() {
        let fixture = fixture();
        let node_id = NodeId::new();
        let credentials = Arc::new(InMemoryEdgeRepository::new());
        credentials
            .create_mcp_credential(credential(&fixture))
            .await
            .expect("store credential");
        let planned = planner(&fixture, node_id, credentials)
            .plan(request(&fixture, node_id))
            .await
            .expect("projection");
        let mut remote = planned.into_projection();
        remote.routes[0].targets[0].node_id = NodeId::new().as_uuid();

        assert!(PlannedMcpGatewayProjection::new(
            node_id,
            remote,
            vec![McpCredentialProjectionVersion::new(
                McpCredentialId::from_uuid(fixture.policy.spec().grants[0].credential_id),
                1,
                1,
            )
            .expect("credential version")],
            now(),
        )
        .expect_err("remote target")
        .contains("another physical node"));
    }

    #[tokio::test]
    async fn rejects_missing_rotated_and_revoked_credentials() {
        let fixture = fixture();
        let node_id = NodeId::new();
        let empty = Arc::new(InMemoryEdgeRepository::new());
        assert!(matches!(
            planner(&fixture, node_id, empty)
                .plan(request(&fixture, node_id))
                .await,
            Err(RepositoryError::Conflict(_))
        ));

        let credentials = Arc::new(InMemoryEdgeRepository::new());
        let mut rotated = credentials
            .create_mcp_credential(credential(&fixture))
            .await
            .expect("store credential");
        rotated
            .rotate(
                "a3s_mcp_def67890abc12345",
                ROTATED_VERIFIER,
                now() + Duration::hours(1),
                now() + Duration::minutes(1),
            )
            .expect("rotate");
        credentials
            .update_mcp_credential(rotated.clone(), 1)
            .await
            .expect("persist rotation");
        assert!(matches!(
            planner(&fixture, node_id, credentials.clone())
                .plan(request(&fixture, node_id))
                .await,
            Err(RepositoryError::Conflict(_))
        ));

        let revoked_credentials = Arc::new(InMemoryEdgeRepository::new());
        let mut revoked = revoked_credentials
            .create_mcp_credential(credential(&fixture))
            .await
            .expect("store credential");
        revoked.revoke(now()).expect("revoke");
        revoked_credentials
            .update_mcp_credential(revoked, 1)
            .await
            .expect("persist revoke");
        assert!(matches!(
            planner(&fixture, node_id, revoked_credentials)
                .plan(request(&fixture, node_id))
                .await,
            Err(RepositoryError::Conflict(_))
        ));
    }
}
