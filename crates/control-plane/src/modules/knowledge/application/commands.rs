use super::{
    AppendKnowledgeBaseCommand, CreateKnowledgeBaseCommand, CreateKnowledgeChunkCommand,
    CreateKnowledgeDocumentCommand, CreateKnowledgePipelineCommand,
    KnowledgeCatalogLifecycleService, KnowledgeDocumentLifecycleService, KnowledgeMutationResult,
    PublishKnowledgePipelineCommand,
};
use crate::modules::knowledge::domain::{
    KnowledgeBaseRecord, KnowledgeChunkRecord, KnowledgeDocumentRecord, KnowledgePipelineRecord,
};
use crate::modules::shared_kernel::application::ApplicationResult;
use a3s_boot::{Command, CommandHandler, CqrsContext};
use std::sync::Arc;

impl Command for CreateKnowledgeBaseCommand {
    type Output = ApplicationResult<KnowledgeMutationResult<KnowledgeBaseRecord>>;
}

pub struct CreateKnowledgeBaseHandler {
    service: Arc<KnowledgeCatalogLifecycleService>,
}

impl CreateKnowledgeBaseHandler {
    pub fn new(service: Arc<KnowledgeCatalogLifecycleService>) -> Self {
        Self { service }
    }
}

impl CommandHandler<CreateKnowledgeBaseCommand> for CreateKnowledgeBaseHandler {
    fn execute(
        &self,
        command: CreateKnowledgeBaseCommand,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<KnowledgeMutationResult<KnowledgeBaseRecord>>>,
    > {
        let service = Arc::clone(&self.service);
        Box::pin(async move { Ok(service.create_knowledge_base(command).await) })
    }
}

impl Command for AppendKnowledgeBaseCommand {
    type Output = ApplicationResult<KnowledgeMutationResult<KnowledgeBaseRecord>>;
}

pub struct AppendKnowledgeBaseHandler {
    service: Arc<KnowledgeCatalogLifecycleService>,
}

impl AppendKnowledgeBaseHandler {
    pub fn new(service: Arc<KnowledgeCatalogLifecycleService>) -> Self {
        Self { service }
    }
}

impl CommandHandler<AppendKnowledgeBaseCommand> for AppendKnowledgeBaseHandler {
    fn execute(
        &self,
        command: AppendKnowledgeBaseCommand,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<KnowledgeMutationResult<KnowledgeBaseRecord>>>,
    > {
        let service = Arc::clone(&self.service);
        Box::pin(async move { Ok(service.append_knowledge_base(command).await) })
    }
}

impl Command for CreateKnowledgePipelineCommand {
    type Output = ApplicationResult<KnowledgeMutationResult<KnowledgePipelineRecord>>;
}

pub struct CreateKnowledgePipelineHandler {
    service: Arc<KnowledgeCatalogLifecycleService>,
}

impl CreateKnowledgePipelineHandler {
    pub fn new(service: Arc<KnowledgeCatalogLifecycleService>) -> Self {
        Self { service }
    }
}

impl CommandHandler<CreateKnowledgePipelineCommand> for CreateKnowledgePipelineHandler {
    fn execute(
        &self,
        command: CreateKnowledgePipelineCommand,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<KnowledgeMutationResult<KnowledgePipelineRecord>>>,
    > {
        let service = Arc::clone(&self.service);
        Box::pin(async move { Ok(service.create_knowledge_pipeline(command).await) })
    }
}

impl Command for PublishKnowledgePipelineCommand {
    type Output = ApplicationResult<KnowledgeMutationResult<KnowledgePipelineRecord>>;
}

pub struct PublishKnowledgePipelineHandler {
    service: Arc<KnowledgeCatalogLifecycleService>,
}

impl PublishKnowledgePipelineHandler {
    pub fn new(service: Arc<KnowledgeCatalogLifecycleService>) -> Self {
        Self { service }
    }
}

impl CommandHandler<PublishKnowledgePipelineCommand> for PublishKnowledgePipelineHandler {
    fn execute(
        &self,
        command: PublishKnowledgePipelineCommand,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<KnowledgeMutationResult<KnowledgePipelineRecord>>>,
    > {
        let service = Arc::clone(&self.service);
        Box::pin(async move { Ok(service.publish_knowledge_pipeline(command).await) })
    }
}

impl Command for CreateKnowledgeDocumentCommand {
    type Output = ApplicationResult<KnowledgeMutationResult<KnowledgeDocumentRecord>>;
}

pub struct CreateKnowledgeDocumentHandler {
    service: Arc<KnowledgeDocumentLifecycleService>,
}

impl CreateKnowledgeDocumentHandler {
    pub fn new(service: Arc<KnowledgeDocumentLifecycleService>) -> Self {
        Self { service }
    }
}

impl CommandHandler<CreateKnowledgeDocumentCommand> for CreateKnowledgeDocumentHandler {
    fn execute(
        &self,
        command: CreateKnowledgeDocumentCommand,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<KnowledgeMutationResult<KnowledgeDocumentRecord>>>,
    > {
        let service = Arc::clone(&self.service);
        Box::pin(async move { Ok(service.create_document(command).await) })
    }
}

impl Command for CreateKnowledgeChunkCommand {
    type Output = ApplicationResult<KnowledgeMutationResult<KnowledgeChunkRecord>>;
}

pub struct CreateKnowledgeChunkHandler {
    service: Arc<KnowledgeDocumentLifecycleService>,
}

impl CreateKnowledgeChunkHandler {
    pub fn new(service: Arc<KnowledgeDocumentLifecycleService>) -> Self {
        Self { service }
    }
}

impl CommandHandler<CreateKnowledgeChunkCommand> for CreateKnowledgeChunkHandler {
    fn execute(
        &self,
        command: CreateKnowledgeChunkCommand,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<KnowledgeMutationResult<KnowledgeChunkRecord>>>,
    > {
        let service = Arc::clone(&self.service);
        Box::pin(async move { Ok(service.create_chunk(command).await) })
    }
}
