use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{validate_single_line, validate_uuid};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeProtocolErrorCode {
    InvalidRequest,
    Unauthenticated,
    Forbidden,
    NotFound,
    Conflict,
    PayloadTooLarge,
    RequestTimeout,
    Unavailable,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeProtocolError {
    pub schema: String,
    pub request_id: Uuid,
    pub code: NodeProtocolErrorCode,
    pub message: String,
    pub retryable: bool,
}

impl NodeProtocolError {
    pub const SCHEMA: &'static str = "a3s.cloud.node-protocol-error.v1";

    pub fn new(
        request_id: Uuid,
        code: NodeProtocolErrorCode,
        message: impl Into<String>,
        retryable: bool,
    ) -> Result<Self, String> {
        let value = Self {
            schema: Self::SCHEMA.into(),
            request_id,
            code,
            message: message.into(),
            retryable,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported node protocol error schema {:?}",
                self.schema
            ));
        }
        validate_uuid("request_id", self.request_id)?;
        validate_single_line("node protocol error message", &self.message, 16 * 1024)
    }
}
