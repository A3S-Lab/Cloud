pub mod domain;
pub mod infrastructure;

pub use domain::{
    ConnectorExecutionError, ConnectorExecutionReceipt, ConnectorExecutionRequest,
    ConnectorHttpMethod, ConnectorHttpStatusPolicy, IConnectorEgressAuthorizer,
    IConnectorExecutionPort,
};
pub use infrastructure::{
    BoundedHttpConnectorExecutor, ResolvedConnectorAuthentication, ResolvedConnectorHttpRevision,
};
