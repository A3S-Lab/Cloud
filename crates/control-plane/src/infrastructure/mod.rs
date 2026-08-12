mod flow;
mod git;
mod immutable_object;
mod oci_registry_client;
mod operation_resource_access;
mod postgres;
mod postgres_schema;
mod vault_client;

#[cfg(test)]
pub(crate) use flow::CLOUD_FLOW_RUNTIME_BUILD_ID;
pub use flow::{
    connect_flow, FlowCoordinatorError, FlowCoordinatorReport, FlowInfrastructure,
    FlowInfrastructureError, FlowOperationCoordinator, FlowRuntimeRouter,
};
pub use postgres::{connect_and_migrate, postgres_health, PostgresBootstrapError};

pub(crate) use git::{GitCommandError, GitCommandRunner};
pub(crate) use immutable_object::{
    ImmutableObjectClient, ImmutableObjectError, ImmutableObjectOpenResult, ImmutableObjectRead,
    ImmutableObjectReader, ImmutableObjectVerification, S3ImmutableObjectOptions,
};
pub(crate) use oci_registry_client::{
    required_registry_header, OciRegistryClient, OciRegistryClientError,
};
pub(crate) use operation_resource_access::OperationResourceAccessResolver;
pub(crate) use postgres::{
    execute, fetch_all, fetch_optional, idempotency_replay, is_foreign_key_violation,
    is_unique_violation, lock_idempotency_key, lock_node_placement, require_one_row, store_audit,
    store_idempotency, store_outbox, transaction_error, AuditWrite, PostgresPersistenceError,
};
pub(crate) use postgres_schema::OutboxEvents;
pub(crate) use vault_client::{VaultClient, VaultClientError};
