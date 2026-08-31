use super::profile::{FunctionModeV1, FunctionOwnerV1};
use super::validation::{validate_digest, validate_single_line, validate_uuid};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const FUNCTION_INVOCATION_FAILURE_SCHEMA_V1: &str = "cloud.function.invocation-failure.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FunctionInvocationFailureCodeV1 {
    InvalidRequest,
    Unauthorized,
    DeadlineExceeded,
    InputTooLarge,
    UnsupportedMediaType,
    ConcurrencyLimited,
    OwnerUnavailable,
    OwnerRejected,
    Cancelled,
    OutputInvalid,
    OutputTooLarge,
    ExternalOutcomeIndeterminate,
    Internal,
}

impl FunctionInvocationFailureCodeV1 {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidRequest => "invalid_request",
            Self::Unauthorized => "unauthorized",
            Self::DeadlineExceeded => "deadline_exceeded",
            Self::InputTooLarge => "input_too_large",
            Self::UnsupportedMediaType => "unsupported_media_type",
            Self::ConcurrencyLimited => "concurrency_limited",
            Self::OwnerUnavailable => "owner_unavailable",
            Self::OwnerRejected => "owner_rejected",
            Self::Cancelled => "cancelled",
            Self::OutputInvalid => "output_invalid",
            Self::OutputTooLarge => "output_too_large",
            Self::ExternalOutcomeIndeterminate => "external_outcome_indeterminate",
            Self::Internal => "internal",
        }
    }

    pub const fn disposition(self) -> FunctionFailureDispositionV1 {
        match self {
            Self::ConcurrencyLimited | Self::OwnerUnavailable => {
                FunctionFailureDispositionV1::CallerPolicy
            }
            Self::ExternalOutcomeIndeterminate => FunctionFailureDispositionV1::Indeterminate,
            Self::InvalidRequest
            | Self::Unauthorized
            | Self::DeadlineExceeded
            | Self::InputTooLarge
            | Self::UnsupportedMediaType
            | Self::OwnerRejected
            | Self::Cancelled
            | Self::OutputInvalid
            | Self::OutputTooLarge
            | Self::Internal => FunctionFailureDispositionV1::Terminal,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FunctionFailureDispositionV1 {
    Terminal,
    CallerPolicy,
    Indeterminate,
}

/// A bounded, provider-neutral failure returned after profile resolution.
/// Retry remains the semantic caller's decision; this value never schedules it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FunctionInvocationFailureV1 {
    pub schema: String,
    pub code: FunctionInvocationFailureCodeV1,
    pub owner: FunctionOwnerV1,
    pub owner_reference_id: Option<Uuid>,
    pub owner_evidence_digest: Option<String>,
    pub message: String,
}

impl FunctionInvocationFailureV1 {
    pub const SCHEMA: &'static str = FUNCTION_INVOCATION_FAILURE_SCHEMA_V1;

    pub fn validate_for_mode(&self, mode: FunctionModeV1) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported Function invocation failure schema {:?}",
                self.schema
            ));
        }
        if self.owner != mode.owner() {
            return Err("Function invocation failure names a foreign lifecycle owner".into());
        }
        if let Some(reference_id) = self.owner_reference_id {
            validate_uuid("Function owner reference ID", reference_id)?;
        }
        if let Some(digest) = &self.owner_evidence_digest {
            validate_digest("Function owner evidence digest", digest)?;
            if self.owner_reference_id.is_none() {
                return Err("Function owner evidence requires its exact owner reference".into());
            }
        }
        validate_single_line("Function invocation failure message", &self.message, 1_024)?;
        if self.code == FunctionInvocationFailureCodeV1::ExternalOutcomeIndeterminate
            && (mode != FunctionModeV1::External || self.owner_reference_id.is_none())
        {
            return Err(
                "indeterminate Function outcome requires one exact external Connector attempt"
                    .into(),
            );
        }
        Ok(())
    }

    pub const fn disposition(&self) -> FunctionFailureDispositionV1 {
        self.code.disposition()
    }
}
