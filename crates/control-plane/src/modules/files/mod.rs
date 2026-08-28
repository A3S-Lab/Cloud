mod application;
mod domain;
mod infrastructure;
mod presentation;

pub use application::{
    ExpireUserFileUpload, ExpireUserFileUploadHandler, GetUserFile, GetUserFileHandler,
    GetUserFileQuota, GetUserFileQuotaHandler, IUserFileObjectStore, ListUserFiles,
    ListUserFilesHandler, RecordUserFileScan, RecordUserFileScanHandler, RecordUserFileUpload,
    RecordUserFileUploadHandler, ReserveUserFile, ReserveUserFileHandler, TombstoneUserFile,
    TombstoneUserFileHandler, UserFileApplicationService, UserFileMutationResult,
    UserFileObjectError, UserFileObjectReader, UserFileTransition, DEFAULT_USER_FILE_LIST_LIMIT,
    MAXIMUM_USER_FILE_LIST_LIMIT,
};
pub use domain::{
    IUserFileRepository, ReserveUserFileWrite, TransitionUserFileWrite, UserFile,
    UserFileAdmissionContract, UserFileAdmissionContractSpec, UserFileContentReference,
    UserFileLifecycleChanged, UserFileObjectWrite, UserFileQuota, UserFileScanDecision,
    UserFileScanPolicy, UserFileScanReceipt, UserFileState,
    DEFAULT_USER_FILE_ORGANIZATION_QUOTA_BYTES, USER_FILE_ADMISSION_CONTRACT_MAX_ACL_BYTES,
    USER_FILE_ADMISSION_CONTRACT_SCHEMA, USER_FILE_LIFECYCLE_EVENT_SCHEMA, USER_FILE_MAX_BYTES,
    USER_FILE_PUBLIC_INTEGER_MAX, USER_FILE_REJECTION_REASON_MAX_BYTES,
    USER_FILE_RETENTION_MAX_DAYS, USER_FILE_UPLOAD_MAX_TTL_SECONDS,
};
#[cfg(test)]
pub(crate) use infrastructure::InMemoryUserFileRepository;
pub use infrastructure::{PostgresUserFileRepository, SharedUserFileObjectStore};
pub(crate) use presentation::{
    FilesModule, UserFileMutationResponse, UserFileQuotaResponse, UserFileResponse,
    USER_FILES_CONTROLLER_PREFIX, USER_FILE_COLLECTION_ROUTE, USER_FILE_ITEM_ROUTE,
    USER_FILE_QUOTA_ROUTE, USER_FILE_TOMBSTONE_ROUTE,
};
