mod certificate;
pub mod commands;
mod gateway_acknowledgement_projector;
mod gateway_snapshot_commands;
mod log_compaction;
mod log_reader;
mod log_retention;
mod node_artifact_authorizer;
pub mod queries;
pub(crate) mod resource_access;
mod runtime_node_evidence;

pub use commands::{
    AcknowledgeNodeCommand, AcknowledgeNodeCommandHandler, AcknowledgeNodeCommandResult,
    ChangeNodeState, ChangeNodeStateHandler, ChangeNodeStateResult, EnqueueNodeCommand,
    EnqueueNodeCommandHandler, EnqueueNodeCommandResult, EnrollNode, EnrollNodeHandler,
    EnrollNodeResult, IssueEnrollmentToken, IssueEnrollmentTokenHandler,
    IssueEnrollmentTokenResult, LeaseNodeCommands, LeaseNodeCommandsHandler, ManageNodePool,
    ManageNodePoolHandler, NegotiateNodeSession, NegotiateNodeSessionHandler,
    NegotiateNodeSessionResult, NodePoolMutation, NodePoolMutationResult,
    RecordGatewayAcknowledgement, RecordGatewayAcknowledgementHandler, RecordNodeLogChunks,
    RecordNodeLogChunksHandler, RecordNodeObservations, RecordNodeObservationsHandler,
    RecordNodeResourceInventory, RecordNodeResourceInventoryHandler, RotateNodeCertificate,
    RotateNodeCertificateHandler, RotateNodeCertificateResult,
};
pub use gateway_acknowledgement_projector::IGatewayAcknowledgementProjector;
pub use gateway_snapshot_commands::{
    FleetGatewayCommandDispatch, FleetGatewayObservationOutcome,
    FleetGatewaySnapshotCommandService, FleetGatewaySnapshotInstallRequest,
    FleetGatewaySnapshotObserveRequest, IFleetGatewaySnapshotCommandPort,
};
pub use log_compaction::LogCompactionWorker;
pub use log_reader::{
    MAX_LOG_PAGE_SIZE, NodeLogGapReason, NodeLogPage, NodeLogReadQuery, NodeLogReader,
    NodeLogRecord,
};
pub use log_retention::LogRetentionWorker;
pub use node_artifact_authorizer::NodeArtifactAuthorizer;
pub use queries::{
    GetNode, GetNodeHandler, GetNodePool, GetNodePoolHandler, ListNodePools, ListNodePoolsHandler,
    ListNodes, ListNodesHandler, NodeQueryResult,
};
pub use resource_access::FleetAccess;
pub(crate) use resource_access::FleetAccessScope;
pub use runtime_node_evidence::{
    IRuntimeNodeEvidenceQueryPort, RuntimeNodeEvidenceQuery, RuntimeNodeEvidenceQueryService,
};

#[cfg(test)]
mod tests;
