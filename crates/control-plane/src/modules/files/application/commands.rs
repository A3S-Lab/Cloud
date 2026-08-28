use super::{
    RecordUserFileScan, RecordUserFileUpload, ReserveUserFile, UserFileApplicationService,
    UserFileMutationResult, UserFileTransition,
};
use crate::modules::shared_kernel::application::ApplicationResult;
use a3s_boot::{Command, CommandHandler, CqrsContext};
use std::sync::Arc;

impl Command for ReserveUserFile {
    type Output = ApplicationResult<UserFileMutationResult>;
}

pub struct ReserveUserFileHandler {
    service: Arc<UserFileApplicationService>,
}

impl ReserveUserFileHandler {
    pub fn new(service: Arc<UserFileApplicationService>) -> Self {
        Self { service }
    }
}

impl CommandHandler<ReserveUserFile> for ReserveUserFileHandler {
    fn execute(
        &self,
        command: ReserveUserFile,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<UserFileMutationResult>>>
    {
        let service = Arc::clone(&self.service);
        Box::pin(async move { Ok(service.reserve(command).await) })
    }
}

pub struct TombstoneUserFile(pub UserFileTransition);

impl Command for TombstoneUserFile {
    type Output = ApplicationResult<UserFileMutationResult>;
}

pub struct TombstoneUserFileHandler {
    service: Arc<UserFileApplicationService>,
}

impl TombstoneUserFileHandler {
    pub fn new(service: Arc<UserFileApplicationService>) -> Self {
        Self { service }
    }
}

impl CommandHandler<TombstoneUserFile> for TombstoneUserFileHandler {
    fn execute(
        &self,
        command: TombstoneUserFile,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<UserFileMutationResult>>>
    {
        let service = Arc::clone(&self.service);
        Box::pin(async move { Ok(service.tombstone(command.0).await) })
    }
}

impl Command for RecordUserFileUpload {
    type Output = ApplicationResult<UserFileMutationResult>;
}

pub struct RecordUserFileUploadHandler {
    service: Arc<UserFileApplicationService>,
}

impl RecordUserFileUploadHandler {
    pub fn new(service: Arc<UserFileApplicationService>) -> Self {
        Self { service }
    }
}

impl CommandHandler<RecordUserFileUpload> for RecordUserFileUploadHandler {
    fn execute(
        &self,
        command: RecordUserFileUpload,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<UserFileMutationResult>>>
    {
        let service = Arc::clone(&self.service);
        Box::pin(async move { Ok(service.record_upload(command).await) })
    }
}

impl Command for RecordUserFileScan {
    type Output = ApplicationResult<UserFileMutationResult>;
}

pub struct RecordUserFileScanHandler {
    service: Arc<UserFileApplicationService>,
}

impl RecordUserFileScanHandler {
    pub fn new(service: Arc<UserFileApplicationService>) -> Self {
        Self { service }
    }
}

impl CommandHandler<RecordUserFileScan> for RecordUserFileScanHandler {
    fn execute(
        &self,
        command: RecordUserFileScan,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<UserFileMutationResult>>>
    {
        let service = Arc::clone(&self.service);
        Box::pin(async move { Ok(service.record_scan(command).await) })
    }
}

pub struct ExpireUserFileUpload(pub UserFileTransition);

impl Command for ExpireUserFileUpload {
    type Output = ApplicationResult<UserFileMutationResult>;
}

pub struct ExpireUserFileUploadHandler {
    service: Arc<UserFileApplicationService>,
}

impl ExpireUserFileUploadHandler {
    pub fn new(service: Arc<UserFileApplicationService>) -> Self {
        Self { service }
    }
}

impl CommandHandler<ExpireUserFileUpload> for ExpireUserFileUploadHandler {
    fn execute(
        &self,
        command: ExpireUserFileUpload,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<UserFileMutationResult>>>
    {
        let service = Arc::clone(&self.service);
        Box::pin(async move { Ok(service.expire_upload(command.0).await) })
    }
}
