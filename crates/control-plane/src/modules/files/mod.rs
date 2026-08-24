mod application;
mod domain;
mod infrastructure;

pub use application::{IUserFileObjectStore, UserFileObjectError, UserFileObjectReader};
pub use domain::{
    UserFile, UserFileAdmissionContract, UserFileAdmissionContractSpec, UserFileContentReference,
    UserFileLifecycleChanged, UserFileObjectWrite, UserFileScanDecision, UserFileScanPolicy,
    UserFileScanReceipt, UserFileState, USER_FILE_ADMISSION_CONTRACT_MAX_ACL_BYTES,
    USER_FILE_ADMISSION_CONTRACT_SCHEMA, USER_FILE_LIFECYCLE_EVENT_SCHEMA, USER_FILE_MAX_BYTES,
    USER_FILE_REJECTION_REASON_MAX_BYTES, USER_FILE_RETENTION_MAX_DAYS,
    USER_FILE_UPLOAD_MAX_TTL_SECONDS,
};
pub use infrastructure::SharedUserFileObjectStore;
