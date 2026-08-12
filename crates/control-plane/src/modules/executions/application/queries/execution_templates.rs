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
    pub limit: usize,
}

impl Query for ListExecutionTemplates {
    type Output = ApplicationResult<Vec<ExecutionTemplateRevision>>;
}

pub struct ListExecutionTemplatesHandler {
    templates: Arc<dyn IExecutionTemplateRepository>,
}

impl ListExecutionTemplatesHandler {
    pub fn new(templates: Arc<dyn IExecutionTemplateRepository>) -> Self {
        Self { templates }
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
        let templates = Arc::clone(&self.templates);
        Box::pin(async move {
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
