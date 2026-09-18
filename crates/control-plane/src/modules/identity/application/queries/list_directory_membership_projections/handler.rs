use super::{
    DirectoryMembershipProjectionListFilter, ListDirectoryMembershipProjections,
};
use crate::modules::identity::domain::entities::DirectoryMembershipProjectionBinding;
use crate::modules::identity::domain::repositories::IDirectoryMembershipProjectionRepository;
use crate::modules::identity::domain::value_objects::DirectoryGrantSubjectRef;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct ListDirectoryMembershipProjectionsHandler {
    repository: Arc<dyn IDirectoryMembershipProjectionRepository>,
}

impl ListDirectoryMembershipProjectionsHandler {
    pub fn new(repository: Arc<dyn IDirectoryMembershipProjectionRepository>) -> Self {
        Self { repository }
    }
}

impl QueryHandler<ListDirectoryMembershipProjections>
    for ListDirectoryMembershipProjectionsHandler
{
    fn execute(
        &self,
        query: ListDirectoryMembershipProjections,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<Vec<DirectoryMembershipProjectionBinding>>>,
    > {
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            match query.filter {
                DirectoryMembershipProjectionListFilter::SubjectRef(subject_ref) => {
                    let subject = match DirectoryGrantSubjectRef::parse(subject_ref) {
                        Ok(value) => value,
                        Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
                    };
                    match repository
                        .list_projection_bindings_for_subject(query.organization_id, &subject)
                        .await
                    {
                        Ok(value) => Ok(Ok(value)),
                        Err(error) => Ok(Err(error.into())),
                    }
                }
                DirectoryMembershipProjectionListFilter::PrincipalId(principal_id) => {
                    match repository
                        .list_projection_bindings_for_principal(
                            query.organization_id,
                            principal_id,
                        )
                        .await
                    {
                        Ok(value) => Ok(Ok(value)),
                        Err(error) => Ok(Err(error.into())),
                    }
                }
            }
        })
    }
}
