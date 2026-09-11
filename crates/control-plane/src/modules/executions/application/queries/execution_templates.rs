use crate::modules::executions::application::{
    ExecutionAccess, ExecutionsProjectScope, IExecutionsProjectAccess,
};
use crate::modules::executions::domain::{ExecutionTemplateRevision, IExecutionTemplateRepository};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    ExecutionTemplateId, ExecutionTemplateRevisionId, OrganizationId, ProjectId,
};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct GetExecutionTemplate {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub template_id: ExecutionTemplateId,
    pub revision_id: ExecutionTemplateRevisionId,
    pub access: ExecutionAccess,
}

impl Query for GetExecutionTemplate {
    type Output = ApplicationResult<ExecutionTemplateRevision>;
}

pub struct GetExecutionTemplateHandler {
    templates: Arc<dyn IExecutionTemplateRepository>,
}

impl GetExecutionTemplateHandler {
    pub fn new(templates: Arc<dyn IExecutionTemplateRepository>) -> Self {
        Self { templates }
    }
}

impl QueryHandler<GetExecutionTemplate> for GetExecutionTemplateHandler {
    fn execute(
        &self,
        query: GetExecutionTemplate,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<ExecutionTemplateRevision>>>
    {
        let templates = Arc::clone(&self.templates);
        Box::pin(async move {
            if !query.access.project_is_visible(query.project_id) {
                return Ok(Err(ApplicationError::NotFound(
                    "execution template revision not found".into(),
                )));
            }
            match templates
                .find(
                    query.organization_id,
                    query.project_id,
                    query.template_id,
                    query.revision_id,
                )
                .await
            {
                Ok(Some(value)) => Ok(Ok(value)),
                Ok(None) => Ok(Err(ApplicationError::NotFound(
                    "execution template revision not found".into(),
                ))),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}

#[derive(Debug, Clone)]
pub struct ListExecutionTemplates {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub access: ExecutionAccess,
    pub limit: usize,
}

impl Query for ListExecutionTemplates {
    type Output = ApplicationResult<Vec<ExecutionTemplateRevision>>;
}

pub struct ListExecutionTemplatesHandler {
    projects: Arc<dyn IExecutionsProjectAccess>,
    templates: Arc<dyn IExecutionTemplateRepository>,
}

impl ListExecutionTemplatesHandler {
    pub fn new(
        projects: Arc<dyn IExecutionsProjectAccess>,
        templates: Arc<dyn IExecutionTemplateRepository>,
    ) -> Self {
        Self {
            projects,
            templates,
        }
    }
}

impl QueryHandler<ListExecutionTemplates> for ListExecutionTemplatesHandler {
    fn execute(
        &self,
        query: ListExecutionTemplates,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<Vec<ExecutionTemplateRevision>>>,
    > {
        let projects = Arc::clone(&self.projects);
        let templates = Arc::clone(&self.templates);
        Box::pin(async move {
            if !query.access.project_is_visible(query.project_id) {
                return Ok(Err(ApplicationError::NotFound("project not found".into())));
            }
            let project_scope =
                match ExecutionsProjectScope::new(query.organization_id, query.project_id) {
                    Ok(scope) => scope,
                    Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
                };
            match projects.project_exists(project_scope).await {
                Ok(true) => {}
                Ok(false) => {
                    return Ok(Err(ApplicationError::NotFound("project not found".into())));
                }
                Err(error) => return Ok(Err(error.into())),
            }
            match templates
                .list(query.organization_id, query.project_id, query.limit)
                .await
            {
                Ok(values) => Ok(Ok(values)),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::executions::application::resource_access::{
        ExecutionAccess, ExecutionAccessScope,
    };
    use crate::modules::executions::infrastructure::InMemoryExecutionTemplateRepository;
    use a3s_boot::ModuleRef;
    use async_trait::async_trait;

    struct DenyAllProjects;

    #[async_trait]
    impl IExecutionsProjectAccess for DenyAllProjects {
        async fn project_exists(
            &self,
            _scope: ExecutionsProjectScope,
        ) -> Result<bool, crate::modules::shared_kernel::domain::RepositoryError> {
            Ok(true)
        }
    }

    #[tokio::test]
    async fn restricted_query_fails_closed_before_getting_an_ungranted_project() {
        let handler =
            GetExecutionTemplateHandler::new(Arc::new(InMemoryExecutionTemplateRepository::new()));
        let result = handler
            .execute(
                GetExecutionTemplate {
                    organization_id: OrganizationId::new(),
                    project_id: ProjectId::new(),
                    template_id: ExecutionTemplateId::new(),
                    revision_id: ExecutionTemplateRevisionId::new(),
                    access: ExecutionAccess::restricted([ExecutionAccessScope::Project {
                        project_id: ProjectId::new(),
                    }]),
                },
                CqrsContext::new(ModuleRef::new()),
            )
            .await
            .expect("handler");
        assert!(matches!(
            result,
            Err(ApplicationError::NotFound(message))
                if message == "execution template revision not found"
        ));
    }

    #[tokio::test]
    async fn restricted_query_fails_closed_before_listing_an_ungranted_project() {
        let handler = ListExecutionTemplatesHandler::new(
            Arc::new(DenyAllProjects),
            Arc::new(InMemoryExecutionTemplateRepository::new()),
        );
        let result = handler
            .execute(
                ListExecutionTemplates {
                    organization_id: OrganizationId::new(),
                    project_id: ProjectId::new(),
                    access: ExecutionAccess::restricted([ExecutionAccessScope::Project {
                        project_id: ProjectId::new(),
                    }]),
                    limit: 10,
                },
                CqrsContext::new(ModuleRef::new()),
            )
            .await
            .expect("handler");
        assert!(matches!(
            result,
            Err(ApplicationError::NotFound(message)) if message == "project not found"
        ));
    }
}
