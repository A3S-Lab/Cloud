use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, canonical_timestamp, OperationId, OrganizationId,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

const MAX_OPERATION_INPUT_BYTES: usize = 128 * 1024;
const MAX_OPERATION_TEXT_BYTES: usize = 512;
const MAX_OPERATION_ERROR_BYTES: usize = 16 * 1024;

/// Exact identity used by Durable Cells when reading the continuation
/// Operation created by Workloads' writer-fence transaction. Operations
/// remains the authority for the request and projection; this value contains
/// only the identity the consumer is willing to accept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCellOperationLookupRequest {
    pub operation_id: OperationId,
    pub organization_id: OrganizationId,
    pub subject_kind: String,
    pub subject_id: Uuid,
    pub workflow_name: String,
    pub workflow_version: String,
}

impl DurableCellOperationLookupRequest {
    pub fn new(
        operation_id: OperationId,
        organization_id: OrganizationId,
        subject_kind: impl Into<String>,
        subject_id: Uuid,
        workflow_name: impl Into<String>,
        workflow_version: impl Into<String>,
    ) -> Self {
        Self {
            operation_id,
            organization_id,
            subject_kind: subject_kind.into(),
            subject_id,
            workflow_name: workflow_name.into(),
            workflow_version: workflow_version.into(),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.operation_id.as_uuid().is_nil()
            || self.organization_id.as_uuid().is_nil()
            || self.subject_id.is_nil()
        {
            return Err("Durable Cell Operation lookup identity is invalid".into());
        }
        validate_text(
            &self.subject_kind,
            MAX_OPERATION_TEXT_BYTES,
            "Durable Cell Operation subject kind",
        )?;
        validate_text(
            &self.workflow_name,
            MAX_OPERATION_TEXT_BYTES,
            "Durable Cell Operation workflow name",
        )?;
        validate_text(
            &self.workflow_version,
            MAX_OPERATION_TEXT_BYTES,
            "Durable Cell Operation workflow version",
        )
    }
}

/// Aggregate-free copy of the immutable Operation request metadata. The
/// input is retained as opaque JSON so the Durable Cells application can
/// validate its own S0 contract without importing an Operations model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCellOperationRequestProjection {
    pub operation_id: OperationId,
    pub organization_id: OrganizationId,
    pub subject_kind: String,
    pub subject_id: Uuid,
    pub workflow_name: String,
    pub workflow_version: String,
    pub input: Value,
    pub requested_at: DateTime<Utc>,
}

impl DurableCellOperationRequestProjection {
    pub fn validate_against(
        &self,
        request: &DurableCellOperationLookupRequest,
    ) -> Result<(), String> {
        request.validate()?;
        if self.operation_id != request.operation_id
            || self.organization_id != request.organization_id
            || self.subject_kind != request.subject_kind
            || self.subject_id != request.subject_id
            || self.workflow_name != request.workflow_name
            || self.workflow_version != request.workflow_version
            || self.requested_at != canonical_timestamp(self.requested_at)
        {
            return Err(
                "Operations returned a request outside the exact Durable Cell scope".into(),
            );
        }
        canonical_json_bounded(
            &self.input,
            MAX_OPERATION_INPUT_BYTES,
            "Durable Cell Operation input",
        )?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurableCellOperationStatus {
    Queued,
    Running,
    Suspended,
    Cancelling,
    Succeeded,
    Failed,
    Cancelled,
}

impl DurableCellOperationStatus {
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

    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

/// Aggregate-free Operation progress returned to Durable Cells. It contains
/// no Operations repository, workflow aggregate, or owner command type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCellOperationProjection {
    pub operation_id: OperationId,
    pub status: DurableCellOperationStatus,
    pub last_sequence: u64,
    pub output: Option<Value>,
    pub error: Option<String>,
    pub updated_at: DateTime<Utc>,
}

impl DurableCellOperationProjection {
    pub fn validate_against(
        &self,
        request: &DurableCellOperationLookupRequest,
    ) -> Result<(), String> {
        request.validate()?;
        if self.operation_id != request.operation_id
            || self.updated_at != canonical_timestamp(self.updated_at)
        {
            return Err(
                "Operations returned a projection outside the exact Durable Cell scope".into(),
            );
        }
        if let Some(output) = &self.output {
            canonical_json_bounded(
                output,
                MAX_OPERATION_INPUT_BYTES,
                "Durable Cell Operation output",
            )?;
        }
        if let Some(error) = &self.error {
            validate_text(
                error,
                MAX_OPERATION_ERROR_BYTES,
                "Durable Cell Operation error",
            )?;
        }
        Ok(())
    }
}

/// Request and optional progress projection read in one owner-boundary call.
/// A missing projection means the Operation is still queued; a missing
/// request is an owner error and is reported by the adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCellOperationSnapshot {
    pub request: DurableCellOperationRequestProjection,
    pub projection: Option<DurableCellOperationProjection>,
}

impl DurableCellOperationSnapshot {
    pub fn validate_against(
        &self,
        request: &DurableCellOperationLookupRequest,
    ) -> Result<(), String> {
        self.request.validate_against(request)?;
        if let Some(projection) = &self.projection {
            projection.validate_against(request)?;
        }
        Ok(())
    }
}

/// Durable Cells' sole application boundary for reading an Operations
/// request/projection pair. Implementations live in an outer anti-corruption
/// adapter; no Operations repository crosses this interface.
#[async_trait]
pub trait IDurableCellOperationPort: Send + Sync {
    async fn load_exact(
        &self,
        request: &DurableCellOperationLookupRequest,
    ) -> ApplicationResult<DurableCellOperationSnapshot>;
}

fn validate_text(value: &str, max_bytes: usize, label: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max_bytes || value.contains(['\0', '\r', '\n']) {
        return Err(format!("{label} is invalid"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::OperationId;

    fn request() -> DurableCellOperationLookupRequest {
        DurableCellOperationLookupRequest::new(
            OperationId::new(),
            OrganizationId::new(),
            "storage_namespace",
            Uuid::new_v4(),
            "cloud.object-namespace.seal",
            "2",
        )
    }

    #[test]
    fn lookup_request_is_bounded_and_exact() {
        let request = request();
        request.validate().expect("valid lookup request");
        let projection = DurableCellOperationRequestProjection {
            operation_id: request.operation_id,
            organization_id: request.organization_id,
            subject_kind: request.subject_kind.clone(),
            subject_id: request.subject_id,
            workflow_name: request.workflow_name.clone(),
            workflow_version: request.workflow_version.clone(),
            input: serde_json::json!({"writerEpoch": 1}),
            requested_at: canonical_timestamp(Utc::now()),
        };
        projection
            .validate_against(&request)
            .expect("valid request projection");
        let mut drifted = request.clone();
        drifted.workflow_version = "3".into();
        assert!(projection.validate_against(&drifted).is_err());
    }

    #[test]
    fn operation_projection_is_locked_to_lookup_identity() {
        let request = request();
        let projection = DurableCellOperationProjection {
            operation_id: request.operation_id,
            status: DurableCellOperationStatus::Succeeded,
            last_sequence: 1,
            output: Some(serde_json::json!({"ok": true})),
            error: None,
            updated_at: canonical_timestamp(Utc::now()),
        };
        projection
            .validate_against(&request)
            .expect("valid projection");
        let mut drifted = projection.clone();
        drifted.operation_id = OperationId::new();
        assert!(drifted.validate_against(&request).is_err());
    }
}
