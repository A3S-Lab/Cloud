use crate::modules::plugins::domain::entities::PluginRegistry;
use crate::modules::plugins::domain::repositories::IPluginRegistryRepository;
use crate::modules::plugins::domain::services::{
    PluginRegistryCatalogError, PluginTrustRootStoreError,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{OrganizationId, PluginRegistryId};

pub(super) async fn find_registry(
    registries: &dyn IPluginRegistryRepository,
    organization_id: OrganizationId,
    registry_id: PluginRegistryId,
) -> ApplicationResult<PluginRegistry> {
    match registries.find(organization_id, registry_id).await {
        Ok(Some(registry)) => Ok(registry),
        Ok(None) => Err(ApplicationError::NotFound(
            "plugin registry not found".into(),
        )),
        Err(error) => Err(error.into()),
    }
}

pub(super) fn map_catalog_error(error: PluginRegistryCatalogError) -> ApplicationError {
    match error {
        PluginRegistryCatalogError::QueryInvalid => {
            ApplicationError::Invalid("plugin catalog query is invalid".into())
        }
        PluginRegistryCatalogError::CursorStale => {
            ApplicationError::Conflict("plugin catalog cursor is stale".into())
        }
        PluginRegistryCatalogError::PackageNotFound => {
            ApplicationError::NotFound("plugin package not found in the verified catalog".into())
        }
        PluginRegistryCatalogError::PackageIncompatible => ApplicationError::Conflict(
            "plugin package is incompatible with the requested host".into(),
        ),
        PluginRegistryCatalogError::Disabled => {
            ApplicationError::Conflict("plugin registry is disabled".into())
        }
        PluginRegistryCatalogError::TrustRoot(PluginTrustRootStoreError::Conflict)
        | PluginRegistryCatalogError::TrustRootEvidenceMismatch => ApplicationError::Conflict(
            "plugin registry trust-root evidence conflicts with enrollment".into(),
        ),
        PluginRegistryCatalogError::Invalid(_)
        | PluginRegistryCatalogError::TrustRoot(PluginTrustRootStoreError::Invalid(_))
        | PluginRegistryCatalogError::TrustRoot(PluginTrustRootStoreError::NotFound)
        | PluginRegistryCatalogError::TrustRoot(PluginTrustRootStoreError::Integrity(_))
        | PluginRegistryCatalogError::TrustRoot(PluginTrustRootStoreError::Storage(_)) => {
            ApplicationError::Unavailable("plugin registry configuration is unavailable".into())
        }
        PluginRegistryCatalogError::Use { code } => {
            ApplicationError::Unavailable(format!("plugin catalog is unavailable ({code})"))
        }
    }
}
