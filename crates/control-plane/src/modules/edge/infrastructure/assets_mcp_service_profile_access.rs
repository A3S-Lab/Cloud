use crate::modules::assets::domain::{IMcpServiceProfileRepository, McpServiceProfile};
use crate::modules::edge::application::{EdgeMcpServiceProfileScope, IEdgeMcpServiceProfileAccess};
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
}

#[async_trait]
impl IEdgeMcpServiceProfileAccess for AssetsEdgeMcpServiceProfileAccessAdapter {
    async fn find_bound_profile(
        &self,
        scope: EdgeMcpServiceProfileScope,
    ) -> Result<Option<McpServiceProfile>, RepositoryError> {
        Ok(self
            .profiles
            .find_mcp_service_profile(
                scope.organization_id(),
                scope.asset_id(),
                scope.asset_release_id(),
            )
            .await?
            .map(|binding| binding.profile))
    }
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
    async fn adapter_returns_only_the_bound_profile_value() {
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
        // created_at must be canonical nanosecond truncation for binding.validate
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
            .expect("find");
        assert_eq!(
            found.as_ref().map(|value| value.digest()),
            Some(profile.digest())
        );
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
