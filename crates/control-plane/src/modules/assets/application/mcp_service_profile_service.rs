use super::catalog_service::{idempotency, validate_request_id};
use crate::modules::assets::domain::{
    BindMcpServiceProfileWrite, IMcpServiceProfileRepository, McpServiceProfile,
    McpServiceProfileBinding, McpServiceProfileBound, McpServiceProfileWrite,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, AssetId, AssetReleaseId, OrganizationId,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

pub struct McpServiceProfileApplicationService {
    profiles: Arc<dyn IMcpServiceProfileRepository>,
}

impl McpServiceProfileApplicationService {
    pub fn new(profiles: Arc<dyn IMcpServiceProfileRepository>) -> Self {
        Self { profiles }
    }

    pub async fn bind(
        &self,
        organization_id: OrganizationId,
        asset_id: AssetId,
        asset_release_id: AssetReleaseId,
        acl: String,
        idempotency_key: String,
        request_id: Uuid,
    ) -> ApplicationResult<McpServiceProfileWrite> {
        validate_request_id(request_id)?;
        let profile = McpServiceProfile::parse_acl(&acl).map_err(ApplicationError::Invalid)?;
        let idempotency = idempotency(
            format!(
                "organizations/{organization_id}/assets/{asset_id}/releases/{asset_release_id}/mcp-service-profile"
            ),
            idempotency_key,
            &serde_json::json!({
                "organization_id": organization_id.as_uuid(),
                "asset_id": asset_id.as_uuid(),
                "asset_release_id": asset_release_id.as_uuid(),
                "profile_digest": profile.digest().as_str(),
            }),
        )?;
        let binding = McpServiceProfileBinding {
            organization_id,
            asset_id,
            asset_release_id,
            profile,
            created_at: canonical_timestamp(Utc::now()),
        };
        let event = McpServiceProfileBound::envelope(&binding, request_id)
            .map_err(ApplicationError::Internal)?;
        self.profiles
            .bind_mcp_service_profile(BindMcpServiceProfileWrite {
                binding,
                event,
                idempotency,
            })
            .await
            .map_err(ApplicationError::from)
    }

    pub async fn get(
        &self,
        organization_id: OrganizationId,
        asset_id: AssetId,
        asset_release_id: AssetReleaseId,
    ) -> ApplicationResult<McpServiceProfileBinding> {
        self.profiles
            .find_mcp_service_profile(organization_id, asset_id, asset_release_id)
            .await
            .map_err(ApplicationError::from)?
            .ok_or_else(|| ApplicationError::NotFound("MCP Service profile not found".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::assets::domain::McpServiceProfileSpec;
    use crate::modules::shared_kernel::domain::RepositoryError;
    use a3s_cloud_contracts::MCP_PROTOCOL_VERSION;
    use async_trait::async_trait;
    use std::sync::Mutex;

    #[derive(Default)]
    struct RecordingProfileRepository {
        writes: Mutex<Vec<BindMcpServiceProfileWrite>>,
    }

    #[async_trait]
    impl IMcpServiceProfileRepository for RecordingProfileRepository {
        async fn bind_mcp_service_profile(
            &self,
            bundle: BindMcpServiceProfileWrite,
        ) -> Result<McpServiceProfileWrite, RepositoryError> {
            bundle.validate().map_err(RepositoryError::Conflict)?;
            let binding = bundle.binding.clone();
            self.writes.lock().expect("profile writes").push(bundle);
            Ok(McpServiceProfileWrite {
                binding,
                replayed: false,
            })
        }

        async fn find_mcp_service_profile(
            &self,
            _organization_id: OrganizationId,
            _asset_id: AssetId,
            _asset_release_id: AssetReleaseId,
        ) -> Result<Option<McpServiceProfileBinding>, RepositoryError> {
            Ok(None)
        }
    }

    #[tokio::test]
    async fn semantically_equivalent_acl_has_one_profile_and_request_digest() {
        let repository = Arc::new(RecordingProfileRepository::default());
        let service = McpServiceProfileApplicationService::new(repository.clone());
        let organization_id = OrganizationId::new();
        let asset_id = AssetId::new();
        let asset_release_id = AssetReleaseId::new();
        let canonical_acl = McpServiceProfile::from_spec(fixture_spec())
            .expect("profile")
            .canonical_acl()
            .to_owned();

        let first = service
            .bind(
                organization_id,
                asset_id,
                asset_release_id,
                canonical_acl.clone(),
                "profile-bind-1".into(),
                Uuid::now_v7(),
            )
            .await
            .expect("first binding");
        let second = service
            .bind(
                organization_id,
                asset_id,
                asset_release_id,
                format!("\n{canonical_acl}\n"),
                "profile-bind-2".into(),
                Uuid::now_v7(),
            )
            .await
            .expect("second binding");

        assert_eq!(first.binding.profile, second.binding.profile);
        let writes = repository.writes.lock().expect("profile writes");
        assert_eq!(writes.len(), 2);
        assert_eq!(
            writes[0].idempotency.request_digest,
            writes[1].idempotency.request_digest
        );
        assert_eq!(writes[0].idempotency.scope, writes[1].idempotency.scope);
    }

    #[tokio::test]
    async fn invalid_acl_is_rejected_before_the_repository() {
        let repository = Arc::new(RecordingProfileRepository::default());
        let service = McpServiceProfileApplicationService::new(repository.clone());

        assert!(matches!(
            service
                .bind(
                    OrganizationId::new(),
                    AssetId::new(),
                    AssetReleaseId::new(),
                    String::new(),
                    "profile-bind-invalid".into(),
                    Uuid::now_v7(),
                )
                .await,
            Err(ApplicationError::Invalid(_))
        ));
        assert!(repository.writes.lock().expect("profile writes").is_empty());
    }

    fn fixture_spec() -> McpServiceProfileSpec {
        McpServiceProfileSpec {
            protocol_versions: vec![MCP_PROTOCOL_VERSION.into()],
            endpoint_path: "/mcp".into(),
            runtime_port: "mcp".into(),
            health_path: "/health".into(),
            request_sse: true,
            subscriptions: true,
            server_discover: true,
            expected_capabilities: vec!["tools".into(), "subscriptions".into()],
            max_request_bytes: 1_048_576,
            max_response_bytes: 8_388_608,
            max_stream_seconds: 3_600,
        }
    }
}
