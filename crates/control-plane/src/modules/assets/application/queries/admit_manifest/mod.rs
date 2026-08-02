use crate::modules::assets::application::AssetGitApplicationService;
use crate::modules::assets::domain::AssetManifestAdmission;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{AssetId, GitCommitSha, OrganizationId};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct AdmitAssetManifest {
    pub organization_id: OrganizationId,
    pub asset_id: AssetId,
    pub commit_sha: GitCommitSha,
}

impl Query for AdmitAssetManifest {
    type Output = ApplicationResult<AssetManifestAdmission>;
}

pub struct AdmitAssetManifestHandler {
    service: Arc<AssetGitApplicationService>,
}

impl AdmitAssetManifestHandler {
    pub fn new(service: Arc<AssetGitApplicationService>) -> Self {
        Self { service }
    }
}

impl QueryHandler<AdmitAssetManifest> for AdmitAssetManifestHandler {
    fn execute(
        &self,
        query: AdmitAssetManifest,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<AssetManifestAdmission>>>
    {
        let service = Arc::clone(&self.service);
        Box::pin(async move {
            Ok(service
                .admit_manifest(query.organization_id, query.asset_id, query.commit_sha)
                .await)
        })
    }
}
