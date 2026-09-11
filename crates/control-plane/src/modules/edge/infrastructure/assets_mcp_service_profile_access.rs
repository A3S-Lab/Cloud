use crate::modules::assets::domain::{
    IMcpServiceProfileRepository, McpServiceProfile, McpServiceProfileBinding,
};
use crate::modules::edge::application::{EdgeMcpServiceProfileScope, IEdgeMcpServiceProfileAccess};
use crate::modules::edge::domain::{
    EdgeMcpServiceProfileAdmission, EdgeMcpServiceProfileProjectionBinding,
};
use crate::modules::shared_kernel::domain::RepositoryError;
use async_trait::async_trait;
use std::sync::Arc;

/// Sole anti-corruption adapter from Edge MCP profile admission to Assets.
#[derive(Clone)]
pub struct AssetsEdgeMcpServiceProfileAccessAdapter {
    profiles: Arc<dyn IMcpServiceProfileRepository>,
}

impl AssetsEdgeMcpServiceProfileAccessAdapter {
    pub fn new(profiles: Arc<dyn IMcpServiceProfileRepository>) -> Self {
        Self { profiles }
    }

    async fn load_binding(
        &self,
        scope: EdgeMcpServiceProfileScope,
    ) -> Result<Option<McpServiceProfileBinding>, RepositoryError> {
        self.profiles
            .find_mcp_service_profile(
                scope.organization_id(),
                scope.asset_id(),
                scope.asset_release_id(),
            )
            .await
    }
}

#[async_trait]
impl IEdgeMcpServiceProfileAccess for AssetsEdgeMcpServiceProfileAccessAdapter {
    async fn find_bound_profile(
        &self,
        scope: EdgeMcpServiceProfileScope,
    ) -> Result<Option<EdgeMcpServiceProfileAdmission>, RepositoryError> {
        Ok(self
            .load_binding(scope)
            .await?
            .map(|binding| admit_mcp_service_profile(&binding.profile))
            .transpose()
            .map_err(RepositoryError::Conflict)?)
    }

    async fn find_projection_binding(
        &self,
        scope: EdgeMcpServiceProfileScope,
    ) -> Result<Option<EdgeMcpServiceProfileProjectionBinding>, RepositoryError> {
        Ok(self
            .load_binding(scope)
            .await?
            .map(|binding| admit_mcp_service_profile_projection_binding(&binding))
            .transpose()
            .map_err(RepositoryError::Conflict)?)
    }
}

pub(crate) fn admit_mcp_service_profile(
    profile: &McpServiceProfile,
) -> Result<EdgeMcpServiceProfileAdmission, String> {
    EdgeMcpServiceProfileAdmission::new(
        profile.digest().clone(),
        profile.spec().endpoint_path.clone(),
        profile.spec().max_request_bytes,
        profile.spec().max_response_bytes,
        profile.spec().max_stream_seconds,
    )
}

pub(crate) fn admit_mcp_service_profile_projection_binding(
    binding: &McpServiceProfileBinding,
) -> Result<EdgeMcpServiceProfileProjectionBinding, String> {
    binding.validate()?;
    let profile = &binding.profile;
    let spec = profile.spec();
    EdgeMcpServiceProfileProjectionBinding::new(
        binding.organization_id,
        binding.asset_id,
        binding.asset_release_id,
        profile.digest().clone(),
        spec.protocol_versions.clone(),
        spec.endpoint_path.clone(),
        spec.runtime_port.clone(),
        spec.health_path.clone(),
        spec.request_sse,
        spec.subscriptions,
        spec.max_request_bytes,
        spec.max_response_bytes,
        binding.created_at,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::assets::domain::{
        BindMcpServiceProfileWrite, McpServiceProfileBinding, McpServiceProfileWrite,
    };
    use crate::modules::shared_kernel::domain::{AssetId, AssetReleaseId, OrganizationId};
    use async_trait::async_trait;
    use std::sync::Mutex;

    struct StubProfiles {
        binding: Mutex<Option<McpServiceProfileBinding>>,
    }

    #[async_trait]
    impl IMcpServiceProfileRepository for StubProfiles {
        async fn bind_mcp_service_profile(
            &self,
            _bundle: BindMcpServiceProfileWrite,
        ) -> Result<McpServiceProfileWrite, RepositoryError> {
            Err(RepositoryError::Storage(
                "bind is out of scope for the Edge MCP profile access adapter".into(),
            ))
        }

        async fn find_mcp_service_profile(
            &self,
            organization_id: OrganizationId,
            asset_id: AssetId,
            asset_release_id: AssetReleaseId,
        ) -> Result<Option<McpServiceProfileBinding>, RepositoryError> {
            let binding = self.binding.lock().expect("lock").clone();
            Ok(binding.filter(|value| {
                value.organization_id == organization_id
                    && value.asset_id == asset_id
                    && value.asset_release_id == asset_release_id
            }))
        }
    }

    #[tokio::test]
    async fn adapter_returns_only_the_bound_admission_fact() {
        use crate::modules::assets::domain::McpServiceProfileSpec;
        use a3s_cloud_contracts::MCP_PROTOCOL_VERSION;
        use chrono::Utc;

        let organization_id = OrganizationId::new();
        let asset_id = AssetId::new();
        let asset_release_id = AssetReleaseId::new();
        let profile = McpServiceProfile::from_spec(McpServiceProfileSpec {
            protocol_versions: vec![MCP_PROTOCOL_VERSION.into()],
            endpoint_path: "/mcp".into(),
            runtime_port: "mcp".into(),
            health_path: "/health".into(),
            request_sse: true,
            subscriptions: true,
            server_discover: true,
            expected_capabilities: vec!["subscriptions".into(), "tools".into()],
            max_request_bytes: 1_048_576,
            max_response_bytes: 8_388_608,
            max_stream_seconds: 3_600,
        })
        .expect("valid MCP Service profile");
        let created_at = crate::modules::shared_kernel::domain::canonical_timestamp(Utc::now());
        let adapter = AssetsEdgeMcpServiceProfileAccessAdapter::new(Arc::new(StubProfiles {
            binding: Mutex::new(Some(McpServiceProfileBinding {
                organization_id,
                asset_id,
                asset_release_id,
                profile: profile.clone(),
                created_at,
            })),
        }));

        let found = adapter
            .find_bound_profile(
                EdgeMcpServiceProfileScope::new(organization_id, asset_id, asset_release_id)
                    .expect("scope"),
            )
            .await
            .expect("find")
            .expect("bound");
        assert_eq!(found.digest(), profile.digest());
        assert_eq!(found.endpoint_path(), "/mcp");
        assert_eq!(found.max_request_bytes(), 1_048_576);
        assert_eq!(found.max_response_bytes(), 8_388_608);
        assert_eq!(found.max_stream_seconds(), 3_600);
        assert!(adapter
            .find_bound_profile(
                EdgeMcpServiceProfileScope::new(organization_id, AssetId::new(), asset_release_id)
                    .expect("missing scope"),
            )
            .await
            .expect("missing")
            .is_none());
    }
}
