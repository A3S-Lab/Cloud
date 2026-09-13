use super::{
    GetKnowledgeBase, GetKnowledgePipeline, KnowledgeCatalogLifecycleService, ListKnowledgeBases,
    ListKnowledgePipelines,
};
use crate::modules::knowledge::domain::{KnowledgeBaseRecord, KnowledgePipelineRecord};
use crate::modules::shared_kernel::application::ApplicationResult;
use a3s_boot::{Query, QueryHandler};
use std::sync::Arc;

impl Query for GetKnowledgeBase {
    type Output = ApplicationResult<KnowledgeBaseRecord>;
}

pub struct GetKnowledgeBaseHandler {
    service: Arc<KnowledgeCatalogLifecycleService>,
}

impl GetKnowledgeBaseHandler {
    pub fn new(service: Arc<KnowledgeCatalogLifecycleService>) -> Self {
        Self { service }
    }
}

impl QueryHandler<GetKnowledgeBase> for GetKnowledgeBaseHandler {
    fn execute(
        &self,
        query: GetKnowledgeBase,
        _context: a3s_boot::CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<KnowledgeBaseRecord>>>
    {
        let service = Arc::clone(&self.service);
        Box::pin(async move { Ok(service.get_knowledge_base(query).await) })
    }
}

impl Query for ListKnowledgeBases {
    type Output = ApplicationResult<Vec<KnowledgeBaseRecord>>;
}

pub struct ListKnowledgeBasesHandler {
    service: Arc<KnowledgeCatalogLifecycleService>,
}

impl ListKnowledgeBasesHandler {
    pub fn new(service: Arc<KnowledgeCatalogLifecycleService>) -> Self {
        Self { service }
    }
}

impl QueryHandler<ListKnowledgeBases> for ListKnowledgeBasesHandler {
    fn execute(
        &self,
        query: ListKnowledgeBases,
        _context: a3s_boot::CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<KnowledgeBaseRecord>>>>
    {
        let service = Arc::clone(&self.service);
        Box::pin(async move { Ok(service.list_knowledge_bases(query).await) })
    }
}

impl Query for GetKnowledgePipeline {
    type Output = ApplicationResult<KnowledgePipelineRecord>;
}

pub struct GetKnowledgePipelineHandler {
    service: Arc<KnowledgeCatalogLifecycleService>,
}

impl GetKnowledgePipelineHandler {
    pub fn new(service: Arc<KnowledgeCatalogLifecycleService>) -> Self {
        Self { service }
    }
}

impl QueryHandler<GetKnowledgePipeline> for GetKnowledgePipelineHandler {
    fn execute(
        &self,
        query: GetKnowledgePipeline,
        _context: a3s_boot::CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<KnowledgePipelineRecord>>>
    {
        let service = Arc::clone(&self.service);
        Box::pin(async move { Ok(service.get_knowledge_pipeline(query).await) })
    }
}

impl Query for ListKnowledgePipelines {
    type Output = ApplicationResult<Vec<KnowledgePipelineRecord>>;
}

pub struct ListKnowledgePipelinesHandler {
    service: Arc<KnowledgeCatalogLifecycleService>,
}

impl ListKnowledgePipelinesHandler {
    pub fn new(service: Arc<KnowledgeCatalogLifecycleService>) -> Self {
        Self { service }
    }
}

impl QueryHandler<ListKnowledgePipelines> for ListKnowledgePipelinesHandler {
    fn execute(
        &self,
        query: ListKnowledgePipelines,
        _context: a3s_boot::CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<Vec<KnowledgePipelineRecord>>>,
    > {
        let service = Arc::clone(&self.service);
        Box::pin(async move { Ok(service.list_knowledge_pipelines(query).await) })
    }
}
