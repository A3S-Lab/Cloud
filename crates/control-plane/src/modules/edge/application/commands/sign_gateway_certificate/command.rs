use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::NodeId;
use a3s_boot::Command;
use a3s_cloud_contracts::{GatewayCertificateSigningRequest, GatewayCertificateSigningResponse};
use chrono::{DateTime, Utc};

#[derive(Clone)]
pub struct SignGatewayCertificate {
    pub authenticated_node_id: NodeId,
    pub request: GatewayCertificateSigningRequest,
    pub received_at: DateTime<Utc>,
}

impl Command for SignGatewayCertificate {
    type Output = ApplicationResult<GatewayCertificateSigningResponse>;
}
