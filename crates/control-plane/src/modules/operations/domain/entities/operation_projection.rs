use crate::modules::shared_kernel::domain::OperationId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationStatus {
    Queued,
    Running,
    Suspended,
    Cancelling,
    Succeeded,
    Failed,
    Cancelled,
}

impl OperationStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Suspended => "suspended",
            Self::Cancelling => "cancelling",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "queued" => Ok(Self::Queued),
            "running" => Ok(Self::Running),
            "suspended" => Ok(Self::Suspended),
            "cancelling" => Ok(Self::Cancelling),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(format!("unknown operation status {value:?}")),
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationProjection {
    pub operation_id: OperationId,
    pub status: OperationStatus,
    pub last_sequence: u64,
    pub output: Option<serde_json::Value>,
    pub error: Option<String>,
    pub updated_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::OperationStatus;

    #[test]
    fn cancelling_status_round_trips_without_becoming_terminal() {
        assert_eq!(
            OperationStatus::parse("cancelling"),
            Ok(OperationStatus::Cancelling)
        );
        assert_eq!(OperationStatus::Cancelling.as_str(), "cancelling");
        assert!(!OperationStatus::Cancelling.is_terminal());
    }
}
