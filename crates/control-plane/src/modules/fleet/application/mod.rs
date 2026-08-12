mod certificate;
pub mod commands;
mod gateway_acknowledgement_projector;
mod log_compaction;
mod log_reader;
mod log_retention;
mod node_artifact_authorizer;
pub mod queries;

pub use commands::{
    AcknowledgeNodeCommand, AcknowledgeNodeCommandHandler, AcknowledgeNodeCommandResult,
    ChangeNodeState, ChangeNodeStateHandler, ChangeNodeStateResult, EnqueueNodeCommand,
    EnqueueNodeCommandHandler, EnqueueNodeCommandResult, EnrollNode, EnrollNodeHandler,
    EnrollNodeResult, IssueEnrollmentToken, IssueEnrollmentTokenHandler,
    IssueEnrollmentTokenResult, LeaseNodeCommands, LeaseNodeCommandsHandler, ManageNodePool,
    ManageNodePoolHandler, NodePoolMutation, NodePoolMutationResult, RecordGatewayAcknowledgement,
    RecordGatewayAcknowledgementHandler, RecordNodeLogChunks, RecordNodeLogChunksHandler,
    RecordNodeObservations, RecordNodeObservationsHandler, RecordNodeResourceInventory,
    RecordNodeResourceInventoryHandler, RotateNodeCertificate, RotateNodeCertificateHandler,
    RotateNodeCertificateResult,
};
pub use gateway_acknowledgement_projector::IGatewayAcknowledgementProjector;
pub use log_compaction::LogCompactionWorker;
pub use log_reader::{
    NodeLogGapReason, NodeLogPage, NodeLogReadQuery, NodeLogReader, NodeLogRecord,
    MAX_LOG_PAGE_SIZE,
};
pub use log_retention::LogRetentionWorker;
pub use node_artifact_authorizer::NodeArtifactAuthorizer;
pub use queries::{
    GetNode, GetNodeHandler, GetNodePool, GetNodePoolHandler, ListNodePools, ListNodePoolsHandler,
    ListNodes, ListNodesHandler, NodeQueryResult,
};

#[cfg(test)]
mod tests;
