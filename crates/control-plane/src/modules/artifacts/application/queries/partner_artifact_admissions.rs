use crate::modules::artifacts::domain::entities::PartnerArtifactAdmission;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::OrganizationId;
use a3s_boot::Query;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct GetPartnerArtifactAdmission {
    pub organization_id: OrganizationId,
    pub admission_id: Uuid,
}

impl Query for GetPartnerArtifactAdmission {
    type Output = ApplicationResult<PartnerArtifactAdmission>;
}

#[derive(Debug, Clone)]
pub struct ListPartnerArtifactAdmissions {
    pub organization_id: OrganizationId,
}

impl Query for ListPartnerArtifactAdmissions {
    type Output = ApplicationResult<Vec<PartnerArtifactAdmission>>;
}

pub struct GetPartnerArtifactAdmissionHandler {
    repository:
        std::sync::Arc<dyn crate::modules::artifacts::domain::IPartnerArtifactAdmissionRepository>,
}

impl GetPartnerArtifactAdmissionHandler {
    pub fn new(
        repository: std::sync::Arc<
            dyn crate::modules::artifacts::domain::IPartnerArtifactAdmissionRepository,
        >,
    ) -> Self {
        Self { repository }
    }
}

impl a3s_boot::QueryHandler<GetPartnerArtifactAdmission> for GetPartnerArtifactAdmissionHandler {
    fn execute(
        &self,
        query: GetPartnerArtifactAdmission,
        _context: a3s_boot::CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<PartnerArtifactAdmission>>>
    {
        let repository = std::sync::Arc::clone(&self.repository);
        Box::pin(async move {
            match repository
                .find_by_id(
                    query.organization_id,
                    crate::modules::artifacts::domain::PartnerArtifactAdmissionId::from_uuid(
                        query.admission_id,
                    ),
                )
                .await
            {
                Ok(Some(admission)) => Ok(Ok(admission)),
                Ok(None) => Ok(Err(
                    crate::modules::shared_kernel::application::ApplicationError::NotFound(
                        "partner artifact admission not found".into(),
                    ),
                )),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}

pub struct ListPartnerArtifactAdmissionsHandler {
    repository:
        std::sync::Arc<dyn crate::modules::artifacts::domain::IPartnerArtifactAdmissionRepository>,
}

impl ListPartnerArtifactAdmissionsHandler {
    pub fn new(
        repository: std::sync::Arc<
            dyn crate::modules::artifacts::domain::IPartnerArtifactAdmissionRepository,
        >,
    ) -> Self {
        Self { repository }
    }
}

impl a3s_boot::QueryHandler<ListPartnerArtifactAdmissions>
    for ListPartnerArtifactAdmissionsHandler
{
    fn execute(
        &self,
        query: ListPartnerArtifactAdmissions,
        _context: a3s_boot::CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<Vec<PartnerArtifactAdmission>>>,
    > {
        let repository = std::sync::Arc::clone(&self.repository);
        Box::pin(async move {
            Ok(repository
                .list_by_organization(query.organization_id)
                .await
                .map_err(crate::modules::shared_kernel::application::ApplicationError::from))
        })
    }
}
