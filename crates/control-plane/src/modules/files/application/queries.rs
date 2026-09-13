use super::{
    GetUserFile, GetUserFileContent, GetUserFileQuota, ListUserFiles, UserFileApplicationService,
    UserFileContentStream,
};
use crate::modules::files::domain::{UserFile, UserFileQuota};
use crate::modules::shared_kernel::application::ApplicationResult;
use a3s_boot::{Query, QueryHandler};
use std::sync::Arc;

impl Query for GetUserFile {
    type Output = ApplicationResult<UserFile>;
}

pub struct GetUserFileHandler {
    service: Arc<UserFileApplicationService>,
}

impl GetUserFileHandler {
    pub fn new(service: Arc<UserFileApplicationService>) -> Self {
        Self { service }
    }
}

impl QueryHandler<GetUserFile> for GetUserFileHandler {
    fn execute(
        &self,
        query: GetUserFile,
        _context: a3s_boot::CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<UserFile>>> {
        let service = Arc::clone(&self.service);
        Box::pin(async move { Ok(service.get(query).await) })
    }
}

impl Query for ListUserFiles {
    type Output = ApplicationResult<Vec<UserFile>>;
}

pub struct ListUserFilesHandler {
    service: Arc<UserFileApplicationService>,
}

impl ListUserFilesHandler {
    pub fn new(service: Arc<UserFileApplicationService>) -> Self {
        Self { service }
    }
}

impl QueryHandler<ListUserFiles> for ListUserFilesHandler {
    fn execute(
        &self,
        query: ListUserFiles,
        _context: a3s_boot::CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<UserFile>>>> {
        let service = Arc::clone(&self.service);
        Box::pin(async move { Ok(service.list(query).await) })
    }
}

impl Query for GetUserFileQuota {
    type Output = ApplicationResult<UserFileQuota>;
}

pub struct GetUserFileQuotaHandler {
    service: Arc<UserFileApplicationService>,
}

impl GetUserFileQuotaHandler {
    pub fn new(service: Arc<UserFileApplicationService>) -> Self {
        Self { service }
    }
}

impl QueryHandler<GetUserFileQuota> for GetUserFileQuotaHandler {
    fn execute(
        &self,
        query: GetUserFileQuota,
        _context: a3s_boot::CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<UserFileQuota>>> {
        let service = Arc::clone(&self.service);
        Box::pin(async move { Ok(service.quota(query).await) })
    }
}

impl Query for GetUserFileContent {
    type Output = ApplicationResult<UserFileContentStream>;
}

pub struct GetUserFileContentHandler {
    service: Arc<UserFileApplicationService>,
}

impl GetUserFileContentHandler {
    pub fn new(service: Arc<UserFileApplicationService>) -> Self {
        Self { service }
    }
}

impl QueryHandler<GetUserFileContent> for GetUserFileContentHandler {
    fn execute(
        &self,
        query: GetUserFileContent,
        _context: a3s_boot::CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<UserFileContentStream>>>
    {
        let service = Arc::clone(&self.service);
        Box::pin(async move { Ok(service.get_content(query).await) })
    }
}
