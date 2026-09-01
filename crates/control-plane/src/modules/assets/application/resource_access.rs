use crate::modules::assets::domain::{Asset, AssetRelease, IAssetRepository};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    AssetId, AssetReleaseId, OrganizationId, RepositoryError,
};
use std::sync::Arc;

/// Assets-owned projection of an already-authorized request.
///
/// Assets are organization-scoped aggregates. Identity's project, environment,
/// and node grants therefore cannot be interpreted as Asset authority. The root
/// Presentation adapter only projects whether the request has organization-wide
/// visibility; Assets applies that closed decision without importing Identity's
/// authorization model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AssetAccess {
    organization_wide: bool,
}

impl AssetAccess {
    pub(crate) const fn organization_wide() -> Self {
        Self {
            organization_wide: true,
        }
    }

    pub(crate) const fn restricted() -> Self {
        Self {
            organization_wide: false,
        }
    }

    pub(crate) const fn organization_catalog_is_visible(self) -> bool {
        self.organization_wide
    }
}

/// Resolves indirect Asset identities without inventing project ownership.
///
/// Asset and AssetRelease are organization-scoped aggregates today. Resource Grants cover only
/// project, environment, and node identities, so restricted memberships fail closed even when
/// they hold one of those grants. Callers may retain their established not-found wording while
/// sharing this single ownership decision across Catalog, hosted Git, and MCP profile surfaces.
#[derive(Clone)]
pub(crate) struct AssetResourceAccess {
    assets: Arc<dyn IAssetRepository>,
}

impl AssetResourceAccess {
    pub fn new(assets: Arc<dyn IAssetRepository>) -> Self {
        Self { assets }
    }

    pub async fn asset(
        &self,
        organization_id: OrganizationId,
        asset_id: AssetId,
        access: &AssetAccess,
        not_found: &'static str,
    ) -> ApplicationResult<Asset> {
        let asset = match self.assets.find_asset(organization_id, asset_id).await {
            Ok(Some(asset)) => asset,
            Ok(None) | Err(RepositoryError::NotFound) => return Err(not_found_error(not_found)),
            Err(error) => return Err(error.into()),
        };
        if !access.organization_catalog_is_visible() {
            return Err(not_found_error(not_found));
        }
        Ok(asset)
    }

    pub async fn release(
        &self,
        organization_id: OrganizationId,
        asset_id: AssetId,
        asset_release_id: AssetReleaseId,
        access: &AssetAccess,
        asset_not_found: &'static str,
        release_not_found: &'static str,
    ) -> ApplicationResult<(Asset, AssetRelease)> {
        let asset = self
            .asset(organization_id, asset_id, access, asset_not_found)
            .await?;
        let release = match self
            .assets
            .find_release(organization_id, asset_id, asset_release_id)
            .await
        {
            Ok(Some(release)) => release,
            Ok(None) | Err(RepositoryError::NotFound) => {
                return Err(not_found_error(release_not_found))
            }
            Err(error) => return Err(error.into()),
        };
        Ok((asset, release))
    }
}

fn not_found_error(message: &'static str) -> ApplicationError {
    ApplicationError::NotFound(message.into())
}

#[cfg(test)]
mod tests {
    use super::AssetAccess;

    #[test]
    fn asset_access_exposes_only_the_organization_catalog_decision() {
        assert!(AssetAccess::organization_wide().organization_catalog_is_visible());
        assert!(!AssetAccess::restricted().organization_catalog_is_visible());
    }
}
