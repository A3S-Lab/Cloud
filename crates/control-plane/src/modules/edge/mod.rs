pub mod application;
pub mod domain;
pub mod infrastructure;
pub mod presentation;

pub use application::{
    GetRoute, GetRouteHandler, ListRoutes, ListRoutesHandler, PublishRoute, PublishRouteHandler,
    PublishRouteResult,
};
pub use domain::repositories::{EdgeRoutePublicationResult, IEdgeRepository};
pub use domain::services::{IGatewayCommandQueue, IRouteTargetReader, RouteTarget};
pub use domain::{
    GatewayPublication, GatewayPublicationState, GatewayScopeState, Route, RouteHostname,
    RoutePath, RoutePortName, RouteState, UpstreamEndpoint,
};
pub use infrastructure::persistence::{InMemoryEdgeRepository, PostgresEdgeRepository};
pub use infrastructure::{
    EdgeGatewayAcknowledgementProjector, FleetGatewayCommandQueue, GatewaySnapshotCompiler,
    GatewaySnapshotCompilerConfig, WorkloadRouteTargetReader,
};
pub use presentation::EdgeModule;
