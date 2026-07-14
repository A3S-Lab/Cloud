use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeProtocolEnvelope {
    pub schema: String,
    pub message_id: Uuid,
    pub node_id: Uuid,
    pub sent_at: DateTime<Utc>,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeHeartbeat {
    pub schema: String,
    pub observed_at: DateTime<Utc>,
    pub agent_version: String,
    pub provider_id: String,
}
