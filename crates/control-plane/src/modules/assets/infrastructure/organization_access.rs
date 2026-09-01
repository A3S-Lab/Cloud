use crate::modules::artifacts::INodeArtifactStore;
use crate::modules::assets::application::{
    AssetCatalogApplicationService, IAssetOrganizationAccess,
};
use crate::modules::assets::domain::{IAssetGitRepository, IAssetRepository};
use crate::modules::identity::domain::repositories::IOrganizationRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::OrganizationId;
use async_trait::async_trait;
use std::sync::Arc;

/// Assets' sole Identity Organization adapter. It validates owner evidence and
/// discards the foreign aggregate at the anti-corruption boundary.
struct IdentityAssetOrganizationAccessAdapter {
    organizations: Arc<dyn IOrganizationRepository>,
}

#[async_trait]
impl IAssetOrganizationAccess for IdentityAssetOrganizationAccessAdapter {
    async fn require_organization(&self, organization_id: OrganizationId) -> ApplicationResult<()> {
        match self.organizations.find(organization_id).await? {
            Some(organization)
                if organization.id == organization_id && organization.aggregate_version > 0 =>
            {
                Ok(())
            }
            Some(_) => Err(ApplicationError::Internal(
                "Identity returned inconsistent Assets organization evidence".into(),
            )),
            None => Err(ApplicationError::NotFound("organization not found".into())),
        }
    }
}

fn organization_access(
    organizations: Arc<dyn IOrganizationRepository>,
) -> Arc<dyn IAssetOrganizationAccess> {
    Arc::new(IdentityAssetOrganizationAccessAdapter { organizations })
}

// Preserve the composition root's repository-oriented constructor while the
// Application service depends only on Assets' consumer-owned port.
impl AssetCatalogApplicationService {
    pub fn new(
        organizations: Arc<dyn IOrganizationRepository>,
        assets: Arc<dyn IAssetRepository>,
        repositories: Arc<dyn IAssetGitRepository>,
        artifacts: Arc<dyn INodeArtifactStore>,
    ) -> Self {
        Self::from_organization_access(
            organization_access(organizations),
            assets,
            repositories,
            artifacts,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::identity::domain::entities::Organization;
    use crate::modules::identity::domain::repositories::{
        CreateOrganizationWrite, ReadOrganizationCatalog,
    };
    use crate::modules::identity::domain::value_objects::OrganizationName;
    use crate::modules::shared_kernel::domain::{IdempotentWrite, RepositoryError};
    use chrono::Utc;

    struct StubOrganizations {
        result: Result<Option<Organization>, RepositoryError>,
    }

    #[async_trait]
    impl IOrganizationRepository for StubOrganizations {
        async fn create(
            &self,
            _write: CreateOrganizationWrite,
        ) -> Result<IdempotentWrite<Organization>, RepositoryError> {
            Err(RepositoryError::Storage(
                "unexpected Organization write".into(),
            ))
        }

        async fn find(
            &self,
            _organization_id: OrganizationId,
        ) -> Result<Option<Organization>, RepositoryError> {
            self.result.clone()
        }

        async fn list_visible(
            &self,
            _read: ReadOrganizationCatalog,
        ) -> Result<Vec<Organization>, RepositoryError> {
            Err(RepositoryError::Storage(
                "unexpected Organization catalog read".into(),
            ))
        }
    }

    fn organizations(
        result: Result<Option<Organization>, RepositoryError>,
    ) -> Arc<dyn IOrganizationRepository> {
        Arc::new(StubOrganizations { result })
    }

    #[tokio::test]
    async fn organization_access_projects_only_consistent_owner_evidence() {
        let organization_id = OrganizationId::new();
        let organization = Organization::create(
            organization_id,
            OrganizationName::parse("assets-owner").expect("Organization name"),
            Utc::now(),
        );
        organization_access(organizations(Ok(Some(organization))))
            .require_organization(organization_id)
            .await
            .expect("existing Organization");

        assert_eq!(
            organization_access(organizations(Ok(None)))
                .require_organization(organization_id)
                .await,
            Err(ApplicationError::NotFound("organization not found".into()))
        );

        let inconsistent = Organization::create(
            OrganizationId::new(),
            OrganizationName::parse("foreign-owner").expect("Organization name"),
            Utc::now(),
        );
        assert!(matches!(
            organization_access(organizations(Ok(Some(inconsistent))))
                .require_organization(organization_id)
                .await,
            Err(ApplicationError::Internal(_))
        ));
        assert_eq!(
            organization_access(organizations(Err(RepositoryError::Storage(
                "Identity unavailable".into(),
            ))))
            .require_organization(organization_id)
            .await,
            Err(ApplicationError::Internal("Identity unavailable".into()))
        );
    }
}
