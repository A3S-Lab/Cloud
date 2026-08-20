mod events;
mod object_store;
mod user_file;
mod user_file_contract;

pub use events::{UserFileLifecycleChanged, USER_FILE_LIFECYCLE_EVENT_SCHEMA};
pub use object_store::{
    IUserFileObjectStore, UserFileObjectError, UserFileObjectReader, UserFileObjectWrite,
};
pub use user_file::{
    UserFile, UserFileScanDecision, UserFileScanReceipt, UserFileState,
    USER_FILE_REJECTION_REASON_MAX_BYTES, USER_FILE_RETENTION_MAX_DAYS,
    USER_FILE_UPLOAD_MAX_TTL_SECONDS,
};
pub use user_file_contract::{
    UserFileAdmissionContract, UserFileAdmissionContractSpec, UserFileContentReference,
    UserFileScanPolicy, USER_FILE_ADMISSION_CONTRACT_MAX_ACL_BYTES,
    USER_FILE_ADMISSION_CONTRACT_SCHEMA, USER_FILE_MAX_BYTES,
};

#[cfg(test)]
mod tests;
