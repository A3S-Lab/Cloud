use super::{
    GetExternalKnowledgeBinding, GetKnowledgeBase, GetKnowledgeChunk, GetKnowledgeDocument,
    GetKnowledgeIndexRevision, GetKnowledgePipeline, GetKnowledgeRetrievalPolicyRevision,
    KnowledgeCatalogLifecycleService, KnowledgeDocumentLifecycleService,
    KnowledgeIndexLifecycleService, ListExternalKnowledgeBindings, ListKnowledgeBases,
    ListKnowledgeChunks, ListKnowledgeDocuments, ListKnowledgeIndexRevisions,
    ListKnowledgePipelines, ListKnowledgeRetrievalPolicyRevisions,
};
use crate::modules::knowledge::domain::{
    ExternalKnowledgeBindingRecord, KnowledgeBaseRecord, KnowledgeChunkRecord,
    KnowledgeDocumentRecord, KnowledgeIndexRevisionRecord, KnowledgePipelineRecord,
    KnowledgeRetrievalPolicyRevisionRecord,
};
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

impl Query for GetKnowledgeDocument {
    type Output = ApplicationResult<KnowledgeDocumentRecord>;
}

pub struct GetKnowledgeDocumentHandler {
    service: Arc<KnowledgeDocumentLifecycleService>,
}

impl GetKnowledgeDocumentHandler {
    pub fn new(service: Arc<KnowledgeDocumentLifecycleService>) -> Self {
        Self { service }
    }
}

impl QueryHandler<GetKnowledgeDocument> for GetKnowledgeDocumentHandler {
    fn execute(
        &self,
        query: GetKnowledgeDocument,
        _context: a3s_boot::CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<KnowledgeDocumentRecord>>>
    {
        let service = Arc::clone(&self.service);
        Box::pin(async move { Ok(service.get_document(query).await) })
    }
}

impl Query for GetKnowledgeChunk {
    type Output = ApplicationResult<KnowledgeChunkRecord>;
}

pub struct GetKnowledgeChunkHandler {
    service: Arc<KnowledgeDocumentLifecycleService>,
}

impl GetKnowledgeChunkHandler {
    pub fn new(service: Arc<KnowledgeDocumentLifecycleService>) -> Self {
        Self { service }
    }
}

impl QueryHandler<GetKnowledgeChunk> for GetKnowledgeChunkHandler {
    fn execute(
        &self,
        query: GetKnowledgeChunk,
        _context: a3s_boot::CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<KnowledgeChunkRecord>>>
    {
        let service = Arc::clone(&self.service);
        Box::pin(async move { Ok(service.get_chunk(query).await) })
    }
}

impl Query for ListKnowledgeDocuments {
    type Output = ApplicationResult<Vec<KnowledgeDocumentRecord>>;
}

pub struct ListKnowledgeDocumentsHandler {
    service: Arc<KnowledgeDocumentLifecycleService>,
}

impl ListKnowledgeDocumentsHandler {
    pub fn new(service: Arc<KnowledgeDocumentLifecycleService>) -> Self {
        Self { service }
    }
}

impl QueryHandler<ListKnowledgeDocuments> for ListKnowledgeDocumentsHandler {
    fn execute(
        &self,
        query: ListKnowledgeDocuments,
        _context: a3s_boot::CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<Vec<KnowledgeDocumentRecord>>>,
    > {
        let service = Arc::clone(&self.service);
        Box::pin(async move { Ok(service.list_documents(query).await) })
    }
}

impl Query for ListKnowledgeChunks {
    type Output = ApplicationResult<Vec<KnowledgeChunkRecord>>;
}

pub struct ListKnowledgeChunksHandler {
    service: Arc<KnowledgeDocumentLifecycleService>,
}

impl ListKnowledgeChunksHandler {
    pub fn new(service: Arc<KnowledgeDocumentLifecycleService>) -> Self {
        Self { service }
    }
}

impl QueryHandler<ListKnowledgeChunks> for ListKnowledgeChunksHandler {
    fn execute(
        &self,
        query: ListKnowledgeChunks,
        _context: a3s_boot::CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<KnowledgeChunkRecord>>>>
    {
        let service = Arc::clone(&self.service);
        Box::pin(async move { Ok(service.list_chunks(query).await) })
    }
}


impl Query for GetKnowledgeIndexRevision {
    type Output = ApplicationResult<KnowledgeIndexRevisionRecord>;
}

pub struct GetKnowledgeIndexRevisionHandler {
    service: Arc<KnowledgeIndexLifecycleService>,
}

impl GetKnowledgeIndexRevisionHandler {
    pub fn new(service: Arc<KnowledgeIndexLifecycleService>) -> Self {
        Self { service }
    }
}

impl QueryHandler<GetKnowledgeIndexRevision> for GetKnowledgeIndexRevisionHandler {
    fn execute(
        &self,
        query: GetKnowledgeIndexRevision,
        _context: a3s_boot::CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<KnowledgeIndexRevisionRecord>>,
    > {
        let service = Arc::clone(&self.service);
        Box::pin(async move { Ok(service.get_index_revision(query).await) })
    }
}

impl Query for GetKnowledgeRetrievalPolicyRevision {
    type Output = ApplicationResult<KnowledgeRetrievalPolicyRevisionRecord>;
}

pub struct GetKnowledgeRetrievalPolicyRevisionHandler {
    service: Arc<KnowledgeIndexLifecycleService>,
}

impl GetKnowledgeRetrievalPolicyRevisionHandler {
    pub fn new(service: Arc<KnowledgeIndexLifecycleService>) -> Self {
        Self { service }
    }
}

impl QueryHandler<GetKnowledgeRetrievalPolicyRevision>
    for GetKnowledgeRetrievalPolicyRevisionHandler
{
    fn execute(
        &self,
        query: GetKnowledgeRetrievalPolicyRevision,
        _context: a3s_boot::CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<KnowledgeRetrievalPolicyRevisionRecord>>,
    > {
        let service = Arc::clone(&self.service);
        Box::pin(async move { Ok(service.get_retrieval_policy_revision(query).await) })
    }
}

impl Query for GetExternalKnowledgeBinding {
    type Output = ApplicationResult<ExternalKnowledgeBindingRecord>;
}

pub struct GetExternalKnowledgeBindingHandler {
    service: Arc<KnowledgeIndexLifecycleService>,
}

impl GetExternalKnowledgeBindingHandler {
    pub fn new(service: Arc<KnowledgeIndexLifecycleService>) -> Self {
        Self { service }
    }
}

impl QueryHandler<GetExternalKnowledgeBinding> for GetExternalKnowledgeBindingHandler {
    fn execute(
        &self,
        query: GetExternalKnowledgeBinding,
        _context: a3s_boot::CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<ExternalKnowledgeBindingRecord>>,
    > {
        let service = Arc::clone(&self.service);
        Box::pin(async move { Ok(service.get_external_binding(query).await) })
    }
}


impl Query for ListKnowledgeIndexRevisions {
    type Output = ApplicationResult<Vec<KnowledgeIndexRevisionRecord>>;
}

pub struct ListKnowledgeIndexRevisionsHandler {
    service: Arc<KnowledgeIndexLifecycleService>,
}

impl ListKnowledgeIndexRevisionsHandler {
    pub fn new(service: Arc<KnowledgeIndexLifecycleService>) -> Self {
        Self { service }
    }
}

impl QueryHandler<ListKnowledgeIndexRevisions> for ListKnowledgeIndexRevisionsHandler {
    fn execute(
        &self,
        query: ListKnowledgeIndexRevisions,
        _context: a3s_boot::CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<Vec<KnowledgeIndexRevisionRecord>>>,
    > {
        let service = Arc::clone(&self.service);
        Box::pin(async move { Ok(service.list_index_revisions(query).await) })
    }
}

impl Query for ListKnowledgeRetrievalPolicyRevisions {
    type Output = ApplicationResult<Vec<KnowledgeRetrievalPolicyRevisionRecord>>;
}

pub struct ListKnowledgeRetrievalPolicyRevisionsHandler {
    service: Arc<KnowledgeIndexLifecycleService>,
}

impl ListKnowledgeRetrievalPolicyRevisionsHandler {
    pub fn new(service: Arc<KnowledgeIndexLifecycleService>) -> Self {
        Self { service }
    }
}

impl QueryHandler<ListKnowledgeRetrievalPolicyRevisions>
    for ListKnowledgeRetrievalPolicyRevisionsHandler
{
    fn execute(
        &self,
        query: ListKnowledgeRetrievalPolicyRevisions,
        _context: a3s_boot::CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<Vec<KnowledgeRetrievalPolicyRevisionRecord>>>,
    > {
        let service = Arc::clone(&self.service);
        Box::pin(async move { Ok(service.list_retrieval_policy_revisions(query).await) })
    }
}

impl Query for ListExternalKnowledgeBindings {
    type Output = ApplicationResult<Vec<ExternalKnowledgeBindingRecord>>;
}

pub struct ListExternalKnowledgeBindingsHandler {
    service: Arc<KnowledgeIndexLifecycleService>,
}

impl ListExternalKnowledgeBindingsHandler {
    pub fn new(service: Arc<KnowledgeIndexLifecycleService>) -> Self {
        Self { service }
    }
}

impl QueryHandler<ListExternalKnowledgeBindings> for ListExternalKnowledgeBindingsHandler {
    fn execute(
        &self,
        query: ListExternalKnowledgeBindings,
        _context: a3s_boot::CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<Vec<ExternalKnowledgeBindingRecord>>>,
    > {
        let service = Arc::clone(&self.service);
        Box::pin(async move { Ok(service.list_external_bindings(query).await) })
    }
}
