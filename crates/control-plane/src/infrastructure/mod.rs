mod flow;
mod git;
mod immutable_object;
mod oci_registry_client;
mod postgres;
mod postgres_schema;
mod vault_client;

pub use flow::{
    connect_flow, FlowCoordinatorError, FlowCoordinatorReport, FlowInfrastructure,
    FlowInfrastructureError, FlowOperationCoordinator, FlowRuntimeRouter,
};
pub use postgres::{connect_and_migrate, postgres_health, PostgresBootstrapError};

pub(crate) use git::{GitCommandError, GitCommandRunner};
pub(crate) use immutable_object::{
    ImmutableObjectClient, ImmutableObjectError, ImmutableObjectOpenResult, ImmutableObjectRead,
    ImmutableObjectVerification, S3ImmutableObjectOptions,
};
pub(crate) use oci_registry_client::{
    required_registry_header, OciRegistryClient, OciRegistryClientError,
};
pub(crate) use postgres::{
    execute, fetch_all, fetch_optional, idempotency_replay, is_foreign_key_violation,
    is_unique_violation, lock_idempotency_key, require_one_row, store_idempotency, store_outbox,
    transaction_error, PostgresPersistenceError,
};
pub(crate) use postgres_schema::OutboxEvents;
pub(crate) use vault_client::{VaultClient, VaultClientError};
