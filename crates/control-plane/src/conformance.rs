//! Non-default persistence conformance assembly.
//!
//! This module is compiled only for retained external persistence gates. It
//! returns owner ports and never publishes concrete Infrastructure adapters as
//! part of the product facade.

use crate::modules::files::{
    IUserFileObjectStore, IUserFileRepository, PostgresUserFileRepository,
    SharedUserFileObjectStore, UserFileAccess, UserFileObjectError,
};
use crate::modules::search::{search_persistence_adapter, ISearchRepository};
use crate::modules::security::{
    security_persistence_adapter, IGatewayRoutePolicyTimelineRepository,
};
use crate::modules::workloads::WorkloadAccess;
use a3s_orm::PostgresExecutor;
use std::{path::PathBuf, sync::Arc};

/// Files persistence ports backed by the production adapters under test.
pub struct UserFilePersistenceConformance {
    pub repository: Arc<dyn IUserFileRepository>,
    pub objects: Arc<dyn IUserFileObjectStore>,
}

/// Builds the exact Files persistence adapters behind their owner ports.
pub fn user_file_persistence_conformance(
    executor: PostgresExecutor,
    object_root: impl Into<PathBuf>,
) -> Result<UserFilePersistenceConformance, UserFileObjectError> {
    Ok(UserFilePersistenceConformance {
        repository: Arc::new(PostgresUserFileRepository::new(executor)),
        objects: Arc::new(SharedUserFileObjectStore::local(object_root)?),
    })
}

/// Creates organization-wide access only for the retained external Files
/// persistence gate. Product composition must derive this projection through
/// the root Presentation ACL instead.
#[doc(hidden)]
pub fn user_file_organization_access_for_conformance() -> UserFileAccess {
    UserFileAccess::organization_wide()
}

/// Creates organization-wide Workloads access only for the retained external
/// Workloads conformance gate. Product composition must derive this projection
/// through the root Presentation ACL instead.
#[doc(hidden)]
pub fn workload_organization_access_for_conformance() -> WorkloadAccess {
    WorkloadAccess::organization_wide()
}

/// Builds the exact production Search persistence adapter through its owner
/// constructor boundary and returns only its domain port.
pub fn search_persistence_conformance(executor: PostgresExecutor) -> Arc<dyn ISearchRepository> {
    search_persistence_adapter(executor)
}

/// Builds the exact production Security investigation persistence adapter
/// through its owner constructor and returns only its domain port.
pub fn security_persistence_conformance(
    executor: PostgresExecutor,
) -> Arc<dyn IGatewayRoutePolicyTimelineRepository> {
    security_persistence_adapter(executor)
}
