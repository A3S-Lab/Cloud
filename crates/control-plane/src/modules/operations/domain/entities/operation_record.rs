use super::{OperationProjection, OperationRequest};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationRecord {
    pub request: OperationRequest,
    pub projection: Option<OperationProjection>,
}
