use super::catalog_service::{idempotency, validate_request_id};
use super::resource_access::AssetResourceAccess;
use crate::modules::assets::domain::{
    BindMcpServiceProfileWrite, IAssetRepository, IMcpServiceProfileRepository, McpServiceProfile,
    McpServiceProfileBinding, McpServiceProfileBound, McpServiceProfileWrite,
};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, AssetId, AssetReleaseId, OrganizationId,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

pub struct McpServiceProfileApplicationService {
    profiles: Arc<dyn IMcpServiceProfileRepository>,
    resource_access: AssetResourceAccess,
}

impl McpServiceProfileApplicationService {
    pub fn new(
        profiles: Arc<dyn IMcpServiceProfileRepository>,
        assets: Arc<dyn IAssetRepository>,
    ) -> Self {
        Self {
            profiles,
            resource_access: AssetResourceAccess::new(assets),
        }
    }

    pub async fn bind(
        &self,
        organization_id: OrganizationId,
        asset_id: AssetId,
        asset_release_id: AssetReleaseId,
        evaluator: &ResourceAccessEvaluator,
        acl: String,
        idempotency_key: String,
        request_id: Uuid,
    ) -> ApplicationResult<McpServiceProfileWrite> {
        validate_request_id(request_id)?;
        let profile = McpServiceProfile::parse_acl(&acl).map_err(ApplicationError::Invalid)?;
        self.resource_access
            .release(
                organization_id,
                asset_id,
                asset_release_id,
                evaluator,
                "Asset not found",
                "Asset release not found",
            )
            .await?;
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
        evaluator: &ResourceAccessEvaluator,
    ) -> ApplicationResult<McpServiceProfileBinding> {
        self.resource_access
            .release(
                organization_id,
                asset_id,
                asset_release_id,
                evaluator,
                "Asset not found",
                "Asset release not found",
            )
            .await?;
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
    use crate::modules::assets::domain::{
        Asset, AssetKind, AssetRelease, AssetReleaseVersion, AssetReleaseWrite, AssetWrite,
        CreateAssetReleaseWrite, CreateAssetWrite, McpServiceProfileSpec,
        TransitionAssetReleaseWrite, TransitionAssetWrite,
    };
    use crate::modules::shared_kernel::domain::{
        GitCommitSha, RepositoryError, ResourceName, Sha256Digest,
    };
    use a3s_cloud_contracts::MCP_PROTOCOL_VERSION;
    use async_trait::async_trait;
    use std::sync::Mutex;

    struct RecordingProfileRepository {
        writes: Mutex<Vec<BindMcpServiceProfileWrite>>,
        asset: Asset,
        release: AssetRelease,
    }

    impl Default for RecordingProfileRepository {
        fn default() -> Self {
            let asset = Asset::create(
                AssetId::new(),
                OrganizationId::new(),
                ResourceName::parse("MCP profile test").expect("Asset name"),
                AssetKind::Mcp,
                Utc::now(),
            )
            .expect("Asset");
            let release = AssetRelease::draft(
                &asset,
                AssetReleaseId::new(),
                AssetReleaseVersion::parse("1.0.0").expect("release version"),
                GitCommitSha::parse("a".repeat(40)).expect("commit SHA"),
                Sha256Digest::parse(format!("sha256:{}", "b".repeat(64))).expect("manifest digest"),
                Utc::now(),
            )
            .expect("Asset release");
            Self {
                writes: Mutex::new(Vec::new()),
                asset,
                release,
            }
        }
    }

    #[async_trait]
    impl IAssetRepository for RecordingProfileRepository {
        async fn create_asset(
            &self,
            _bundle: CreateAssetWrite,
        ) -> Result<AssetWrite, RepositoryError> {
            Err(RepositoryError::NotFound)
        }

        async fn transition_asset(
            &self,
            _bundle: TransitionAssetWrite,
        ) -> Result<AssetWrite, RepositoryError> {
            Err(RepositoryError::NotFound)
        }

        async fn find_asset(
            &self,
            organization_id: OrganizationId,
            asset_id: AssetId,
        ) -> Result<Option<Asset>, RepositoryError> {
            Ok(
                (self.asset.organization_id == organization_id && self.asset.id == asset_id)
                    .then(|| self.asset.clone()),
            )
        }

        async fn list_assets(
            &self,
            organization_id: OrganizationId,
        ) -> Result<Vec<Asset>, RepositoryError> {
            Ok((self.asset.organization_id == organization_id)
                .then(|| self.asset.clone())
                .into_iter()
                .collect())
        }

        async fn create_release(
            &self,
            _bundle: CreateAssetReleaseWrite,
        ) -> Result<AssetReleaseWrite, RepositoryError> {
            Err(RepositoryError::NotFound)
        }

        async fn transition_release(
            &self,
            _bundle: TransitionAssetReleaseWrite,
        ) -> Result<AssetReleaseWrite, RepositoryError> {
            Err(RepositoryError::NotFound)
        }

        async fn find_release(
            &self,
            organization_id: OrganizationId,
            asset_id: AssetId,
            asset_release_id: AssetReleaseId,
        ) -> Result<Option<AssetRelease>, RepositoryError> {
            Ok((self.release.organization_id == organization_id
                && self.release.asset_id == asset_id
                && self.release.id == asset_release_id)
                .then(|| self.release.clone()))
        }

        async fn list_releases(
            &self,
            organization_id: OrganizationId,
            asset_id: AssetId,
        ) -> Result<Vec<AssetRelease>, RepositoryError> {
            Ok((self.release.organization_id == organization_id
                && self.release.asset_id == asset_id)
                .then(|| self.release.clone())
                .into_iter()
                .collect())
        }
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
        let service =
            McpServiceProfileApplicationService::new(repository.clone(), repository.clone());
        let organization_id = repository.asset.organization_id;
        let asset_id = repository.asset.id;
        let asset_release_id = repository.release.id;
        let resource_access = ResourceAccessEvaluator::organization_wide();
        let canonical_acl = McpServiceProfile::from_spec(fixture_spec())
            .expect("profile")
            .canonical_acl()
            .to_owned();

        let first = service
            .bind(
                organization_id,
                asset_id,
                asset_release_id,
                &resource_access,
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
                &resource_access,
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
        let service =
            McpServiceProfileApplicationService::new(repository.clone(), repository.clone());

        assert!(matches!(
            service
                .bind(
                    OrganizationId::new(),
                    AssetId::new(),
                    AssetReleaseId::new(),
                    &ResourceAccessEvaluator::organization_wide(),
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
