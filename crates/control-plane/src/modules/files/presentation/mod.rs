mod controller;
mod dto;
mod files_module;

pub const USER_FILES_CONTROLLER_PREFIX: &str = "/organizations";
pub const USER_FILE_COLLECTION_ROUTE: &str = "/{organization_id}/projects/{project_id}/user-files";
pub const USER_FILE_ITEM_ROUTE: &str =
    "/{organization_id}/projects/{project_id}/user-files/{user_file_id}";
pub const USER_FILE_TOMBSTONE_ROUTE: &str =
    "/{organization_id}/projects/{project_id}/user-files/{user_file_id}/tombstone";
pub const USER_FILE_QUOTA_ROUTE: &str = "/{organization_id}/user-file-quota";

pub use controller::{user_file_commands_controller, user_file_queries_controller};
pub use dto::{
    ReserveUserFileRequest, TombstoneUserFileRequest, UserFileMutationResponse,
    UserFileQuotaResponse, UserFileResponse,
};
pub use files_module::FilesModule;
